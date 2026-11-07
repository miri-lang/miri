// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering every body a program needs: the program's own functions and
//! methods, the bodies its imports bring in, each generic function and class
//! method at every instantiation the program reaches, inherited and trait
//! default methods, element equality and hash bodies, and residency
//! specializations.

use super::generic_instantiations::{refuse_unnameable_generic_calls, GenericMethodBodies};
use super::*;

/// One generic-class instantiation's substitution: the generic-name→concrete-type
/// map used to substitute a method body, paired with the ordered name/type pairs
/// used to mangle the per-instantiation symbol.
type InstantiationSub = (std::collections::HashMap<String, Type>, Vec<(String, Type)>);

/// A failure lowering the per-instantiation body `symbol`.
///
/// A refusal of the program is reported as the diagnostic it is; anything
/// else is a failure of the compiler's own, reported against the body.
pub(super) fn monomorphized_lowering_failure(
    symbol: &Symbol,
    error: crate::error::lowering::LoweringError,
) -> CompilerError {
    if refuses_the_program(error.kind.properties().code) {
        return CompilerError::Lowering(error);
    }
    // Still the compiler's own failure, but reported where lowering stopped,
    // so an author is pointed at the construct rather than at nothing.
    let source = error.source_override.as_deref().cloned();
    CompilerError::Lowering(
        crate::error::lowering::LoweringError::internal(
            DiagnosticCode::CgInternalCodegenError,
            format!("MIR lowering failed for {}: {}", symbol.written(), error),
            error.span,
        )
        .in_source(source),
    )
}

/// Whether a lowering error under `code` refuses the program, rather than
/// reporting a gap or a broken invariant in the compiler itself.
///
/// The program is refused for a mistake its author can fix: a type error
/// only lowering can see (a GPU buffer, launch shape or atomic of the wrong
/// kind), code a target cannot run as written, an instantiation its arguments
/// do not name or one it needs past a bound, and the structural errors the
/// type checker normally refuses first. A construct lowering does not support,
/// a type lowering cannot find and an invalid body are the compiler's own.
fn refuses_the_program(code: DiagnosticCode) -> bool {
    match code.area() {
        "TYP" | "TAR" => true,
        "MIR" => matches!(
            code,
            DiagnosticCode::MirUndefinedVariable
                | DiagnosticCode::MirBreakOutsideLoop
                | DiagnosticCode::MirContinueOutsideLoop
                | DiagnosticCode::MirInvalidGpuLaunchArguments
                | DiagnosticCode::MirMissingStructField
                | DiagnosticCode::MirPolymorphicRecursion
                | DiagnosticCode::MirInvalidInstantiationArgument
        ),
        _ => false,
    }
}

fn called_functions(bodies: &[LoweredBody]) -> HashSet<Symbol> {
    bodies
        .iter()
        .flat_map(|lowered| direct_callees(&lowered.body))
        .cloned()
        .collect()
}

/// The functions the lowered bodies call, gathered as they grow: each refresh
/// scans only the bodies lowered since the one before.
///
/// A body that constructs a class instance calls, through the instance's
/// vtable, the body each slot of it a virtual call reads names, so those count
/// as called too; the vtable demand says which, from the reached bodies alone.
struct CalledFunctions {
    names: HashSet<Symbol>,
    /// The functions a lowered body calls outright, not through a vtable slot.
    direct: HashSet<Symbol>,
    vtables: mir::lowering::vtable_demand::VtableDemand,
    scanned: usize,
}

impl CalledFunctions {
    /// Nothing called yet, with the program's entry point and the bodies
    /// codegen names outside any call counted as reached.
    fn new(reach: &ReachTables) -> Self {
        let roots = reach
            .synthesized
            .iter()
            .cloned()
            .chain(std::iter::once(Symbol::entry()));
        CalledFunctions {
            names: HashSet::new(),
            direct: HashSet::new(),
            vtables: mir::lowering::vtable_demand::VtableDemand::rooted_at(roots),
            scanned: 0,
        }
    }

    /// Scan the bodies lowered since the last refresh; fails where they reach
    /// an instance past the bounds on instantiations.
    fn refresh(
        &mut self,
        bodies: &[LoweredBody],
        result: &PipelineResult,
        reach: &ReachTables,
    ) -> Result<(), CompilerError> {
        let unscanned = bodies.get(self.scanned..).unwrap_or_default();
        let direct = called_functions(unscanned);
        self.names.extend(direct.iter().cloned());
        self.direct.extend(direct);
        self.vtables
            .record(
                unscanned
                    .iter()
                    .map(|lowered| (&lowered.symbol, &lowered.body)),
                mir::lowering::vtable_demand::DemandTables {
                    type_checker: &result.type_checker,
                    layout: &reach.vtable_layout,
                },
            )
            .map_err(CompilerError::Lowering)?;
        self.names.extend(self.vtables.take_named_slot_symbols());
        self.scanned = bodies.len();
        Ok(())
    }

    fn contains(&self, symbol: &Symbol) -> bool {
        self.names.contains(symbol)
    }

    /// Whether a lowered body calls `symbol` outright.
    fn is_called(&self, symbol: &Symbol) -> bool {
        self.direct.contains(symbol)
    }
}

/// The function the top-level declaration `decl`, made by `stmt`, declares, as
/// a call that resolved to it identifies it.
fn declared_function(
    result: &PipelineResult,
    stmt: &Statement,
    decl: &FunctionDeclarationData,
) -> Result<DeclaredFunction, CompilerError> {
    Ok(DeclaredFunction {
        module: declaring_module(result, stmt, &decl.name)?.clone(),
        name: decl.name.clone(),
    })
}

/// The module the top-level function `name`, declared by `stmt`, is written
/// in, as the type checker recorded it.
///
/// Every top-level function this compilation lowers was recorded where it was
/// checked, so a declaration with no recorded module was never checked;
/// guessing a module for it would link it under some other function's name,
/// so it is refused as an internal error instead.
fn declaring_module<'r>(
    result: &'r PipelineResult,
    stmt: &Statement,
    name: &str,
) -> Result<&'r ModuleId, CompilerError> {
    result
        .type_checker
        .declaring_module(stmt.id)
        .ok_or_else(|| {
            CompilerError::Lowering(LoweringError::internal(
                DiagnosticCode::MirUndefinedVariable,
                format!("no declaring module was recorded for the function `{name}`"),
                stmt.span,
            ))
        })
}

/// The method declaration `stmt` holds, when it has a body shared by every
/// call: one it gives a body, declaring no type parameters of its own. A
/// generic method (`fn m<T>`) is given no shared body, which would lay out
/// values of a placeholder no call settles; each call's copy is lowered by
/// the generic instantiation worklist instead.
fn shared_method_body(stmt: &Statement) -> Option<&FunctionDeclarationData> {
    let StatementKind::FunctionDeclaration(decl) = &stmt.node else {
        return None;
    };
    (decl.body.is_some() && !decl.declares_type_parameters()).then_some(decl)
}

/// Each of `methods` with a body shared by every call, with its declaration,
/// in order: what every per-type lowering loop compiles under the type's name.
fn shared_method_bodies<'s>(
    methods: impl IntoIterator<Item = &'s Statement>,
) -> impl Iterator<Item = (&'s Statement, &'s FunctionDeclarationData)> {
    methods
        .into_iter()
        .filter_map(|stmt| shared_method_body(stmt).map(|decl| (stmt, decl)))
}

/// Every top-level function declaration `keep` selects, by the function it
/// declares, with where its statement sits.
///
/// Two statements declaring one function mean the module that declares one
/// of them was lost on the way; keeping either would run its body wherever
/// the other is called, so that is refused instead.
fn function_declarations(
    result: &PipelineResult,
    keep: impl Fn(&FunctionDeclarationData) -> bool,
) -> Result<std::collections::HashMap<DeclaredFunction, StatementSite>, CompilerError> {
    let mut declarations = std::collections::HashMap::new();
    for (site, stmt) in statements_with_sites(result) {
        let StatementKind::FunctionDeclaration(decl) = &stmt.node else {
            continue;
        };
        if !keep(decl) {
            continue;
        }
        match declarations.entry(declared_function(result, stmt, decl)?) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(site);
            }
            std::collections::hash_map::Entry::Occupied(slot) => {
                return Err(CompilerError::Lowering(LoweringError::internal(
                    DiagnosticCode::MirSymbolCollision,
                    format!("two declarations were both taken for `{}`", slot.key().name),
                    stmt.span,
                )));
            }
        }
    }
    Ok(declarations)
}

/// Where a top-level statement sits: in the program's own body or among the
/// imported statements. Held in place of a reference so a table built from
/// the statements outlives the registry updates between lowering rounds,
/// which borrow the type checker mutably.
#[derive(Clone, Copy)]
pub(super) enum StatementSite {
    Program(usize),
    Imported(usize),
}

impl StatementSite {
    pub(super) fn statement(self, result: &PipelineResult) -> Option<&Statement> {
        match self {
            StatementSite::Program(index) => result.ast.body.get(index),
            StatementSite::Imported(index) => result.type_checker.imported_statements.get(index),
        }
    }
}

/// Every top-level statement with its site, the program's own before the
/// imported ones.
pub(super) fn statements_with_sites(
    result: &PipelineResult,
) -> impl Iterator<Item = (StatementSite, &Statement)> {
    let program = result.ast.body.iter().enumerate();
    let imported = result.type_checker.imported_statements.iter().enumerate();
    program
        .map(|(index, stmt)| (StatementSite::Program(index), stmt))
        .chain(imported.map(|(index, stmt)| (StatementSite::Imported(index), stmt)))
}

/// A method of a generic class instance a lowered body reaches that is not
/// compiled yet.
struct WantedMethod {
    class_site: StatementSite,
    /// Where the method sits among the ones the class compiles under its own
    /// name.
    method_index: usize,
    method: InstanceMethod,
    demand: MethodDemand,
}

/// One method a class or generic enum compiles under its own name, at one of
/// its instantiations, with the statement declaring the type.
struct InstanceMethodSite<'r> {
    class_site: StatementSite,
    class_stmt: &'r Statement,
    method_index: usize,
    method: InstanceMethod,
}

/// One default body a trait declares: the trait, the method, its shared
/// `{Trait}_{method}` symbol and where the declaring trait sits.
struct TraitDefaultBody {
    trait_name: String,
    symbol: Symbol,
    trait_site: StatementSite,
    member: usize,
}

impl TraitDefaultBody {
    /// The declaring trait statement and the method statement within it.
    fn statements<'r>(&self, result: &'r PipelineResult) -> Option<(&'r Statement, &'r Statement)> {
        let trait_stmt = self.trait_site.statement(result)?;
        let StatementKind::Trait(_, _, _, methods, _) = &trait_stmt.node else {
            return None;
        };
        Some((trait_stmt, methods.get(self.member)?))
    }
}

/// Every default body the program's traits and the imported ones declare,
/// found by one scan. The first declaration of a trait's method wins, so a
/// program trait shadows an imported trait of the same name.
pub(super) struct TraitDefaultBodies {
    bodies: Vec<TraitDefaultBody>,
    by_trait: std::collections::HashMap<String, std::collections::HashMap<String, usize>>,
}

impl TraitDefaultBodies {
    fn collect(result: &PipelineResult) -> Self {
        let mut table = TraitDefaultBodies {
            bodies: Vec::new(),
            by_trait: std::collections::HashMap::new(),
        };
        for (trait_site, stmt) in statements_with_sites(result) {
            let StatementKind::Trait(name_expr, _, _, methods, _) = &stmt.node else {
                continue;
            };
            let Some(trait_name) = result.type_checker.type_identity_named_by(name_expr) else {
                continue;
            };
            for (member, method_stmt) in methods.iter().enumerate() {
                let StatementKind::FunctionDeclaration(decl) = &method_stmt.node else {
                    continue;
                };
                if decl.body.is_some() {
                    table.insert(trait_name, &decl.name, trait_site, member);
                }
            }
        }
        table
    }

    fn insert(
        &mut self,
        trait_name: &str,
        method_name: &str,
        trait_site: StatementSite,
        member: usize,
    ) {
        let methods = self.by_trait.entry(trait_name.to_string()).or_default();
        if methods.contains_key(method_name) {
            return;
        }
        methods.insert(method_name.to_string(), self.bodies.len());
        self.bodies.push(TraitDefaultBody {
            trait_name: trait_name.to_string(),
            symbol: Symbol::method(trait_name, &[], method_name, &[]),
            trait_site,
            member,
        });
    }

    /// The method statement of `trait_name`'s default `method_name`.
    pub(super) fn method<'r>(
        &self,
        result: &'r PipelineResult,
        trait_name: &str,
        method_name: &str,
    ) -> Option<&'r Statement> {
        let index = *self.by_trait.get(trait_name)?.get(method_name)?;
        let (_, method_stmt) = self.bodies.get(index)?.statements(result)?;
        Some(method_stmt)
    }
}

/// One method a generic class declares, compiled under the class's own name:
/// its open template, which leaves the class's parameters open.
///
/// Every call at concrete arguments names the body compiled for its
/// instantiation, so a template is compiled only where something still names
/// it — a shared generic body handing its own parameters on, or a vtable built
/// where the arguments have no spelling. Compiling one nothing names would lay
/// out values of parameters no instantiation settles.
struct ClassTemplateMethod {
    class_name: String,
    symbol: Symbol,
    class_site: StatementSite,
    member: usize,
}

impl ClassTemplateMethod {
    /// The declaring class statement and the method statement within it.
    fn statements<'r>(&self, result: &'r PipelineResult) -> Option<(&'r Statement, &'r Statement)> {
        let class_stmt = self.class_site.statement(result)?;
        let StatementKind::Class(class_data) = &class_stmt.node else {
            return None;
        };
        Some((class_stmt, class_data.body.get(self.member)?))
    }
}

/// The method bodies every generic class of the program and its imports
/// declares, the program's own first.
fn class_template_methods(result: &PipelineResult) -> Vec<ClassTemplateMethod> {
    let mut templates = Vec::new();
    for (class_site, stmt) in statements_with_sites(result) {
        let StatementKind::Class(class_data) = &stmt.node else {
            continue;
        };
        if class_data.generics.is_none() {
            continue;
        }
        let Some(class_name) = result.type_checker.type_identity_named_by(&class_data.name) else {
            continue;
        };
        for (member, method_stmt) in class_data.body.iter().enumerate() {
            let Some(decl) = shared_method_body(method_stmt) else {
                continue;
            };
            templates.push(ClassTemplateMethod {
                class_name: class_name.to_string(),
                symbol: Symbol::method(class_name, &[], &decl.name, &[]),
                class_site,
                member,
            });
        }
    }
    templates
}

/// What every lowering round reads and no round changes: the generic function
/// declarations, the trait default bodies, and the symbols codegen names
/// outside any call. Built once, before the first body is lowered.
pub(super) struct ReachTables {
    /// Every generic function declaration, by the module that declares it
    /// and its name.
    pub(super) generic_functions: std::collections::HashMap<DeclaredFunction, StatementSite>,
    /// Every generic method body a class or enum declares.
    pub(super) generic_methods: GenericMethodBodies,
    pub(super) trait_defaults: TraitDefaultBodies,
    class_templates: Vec<ClassTemplateMethod>,
    synthesized: HashSet<Symbol>,
    /// The slot numbering the lowered virtual calls index by.
    vtable_layout: mir::lowering::dispatch_symbols::VtableLayout,
}

impl ReachTables {
    fn collect(result: &PipelineResult) -> Result<Self, CompilerError> {
        Ok(ReachTables {
            generic_functions: function_declarations(result, |decl| decl.generics.is_some())?,
            generic_methods: GenericMethodBodies::collect(result),
            trait_defaults: TraitDefaultBodies::collect(result),
            class_templates: class_template_methods(result),
            synthesized: mir::lowering::dispatch_symbols::synthesized_references(
                result.type_checker.type_definitions(),
            ),
            vtable_layout: mir::lowering::dispatch_symbols::VtableLayout::of(
                result.type_checker.type_definitions(),
            ),
        })
    }
}

/// Fill the generic-class instantiation registry with everything the program
/// needs a per-instantiation body for, whether or not it names it.
///
/// Fails where a class's fields nest an instantiation past the bounds on
/// instantiations.
pub(super) fn complete_generic_instantiation_registry(
    type_checker: &mut TypeChecker,
    ast: &Program,
) -> Result<(), CompilerError> {
    record_inferred_generic_instantiations(type_checker);
    record_pinned_base_class_instantiations(type_checker);
    expand_nested_generic_instantiations(type_checker, ast)
}

/// Record the instantiation each class that declares no parameters of its own
/// reaches its base at.
///
/// `class Child extends Base<String>` writes the parent's type argument down
/// without carrying one, so the child appears in no instantiation tuple and
/// nothing derives `Base<String>` from it. Every body such a child inherits —
/// its `super.init`, the methods a call on it reaches — belongs to that
/// instantiation of the parent, and a non-generic class's own bodies are always
/// lowered, so the parent's must exist for them to call.
///
/// Discoveries are recorded in a fixed order: the definitions come out of a
/// hash map, and the registry's order decides the order bodies are emitted in,
/// which a byte-reproducible build depends on.
fn record_pinned_base_class_instantiations(type_checker: &mut TypeChecker) {
    let mut discovered: Vec<(String, Vec<Type>)> = type_checker
        .type_definitions()
        .iter()
        .filter(|(_, def)| matches!(def, TypeDefinition::Class(class) if class.generics.is_none()))
        .filter_map(|(class_name, _)| {
            mir::lowering::inherited_instantiation::base_class_instantiation(
                type_checker.type_definitions(),
                class_name,
                &[],
            )
        })
        .filter(|(_, args)| {
            args.iter()
                .all(|arg| mir::lowering::has_a_monomorphized_spelling(&arg.kind))
        })
        .collect();
    discovered.sort_by_cached_key(|(name, args)| TypeInstance::new(name, args));
    for (name, args) in discovered {
        type_checker.record_generic_class_instantiation(&name, args);
    }
}

/// Record the instantiations a program reaches only through a return type.
///
/// The registry is filled where a program writes a type argument down, and a
/// collection's inherited transforms are declared to return `[T]` — so
/// `rows.reversed()` on an `Array<W, 3>` produces a `List<W>` that appears
/// nowhere in the source. Without it recorded, every call on that value falls
/// back to the shared generic body, which stores each element without taking a
/// reference while the call site releases every one of them. The types the
/// checker inferred are where such an instantiation does appear.
///
/// Discoveries are recorded in a fixed order: they come out of a hash map, and
/// the registry's order decides the order bodies are emitted in, which a
/// byte-reproducible build depends on.
fn record_inferred_generic_instantiations(type_checker: &mut TypeChecker) {
    let mut discovered: Vec<(String, Vec<Type>)> = Vec::new();
    for ty in type_checker.inferred_types() {
        mir::lowering::class_instantiations::collect_generic_instantiations(
            type_checker,
            &ty.kind,
            &mut discovered,
        );
    }
    discovered.sort_by_cached_key(|(name, args)| TypeInstance::new(name, args));
    for (name, args) in discovered {
        type_checker.record_generic_class_instantiation(&name, args);
    }
}

/// Add to the registry the instantiations lowered bodies recorded that it does
/// not hold yet, with the instantiations those reach through their fields.
///
/// Every one of them is held to the bounds on instantiations first, and a
/// chain that nests deeper than anything registered is followed one
/// instantiation per round (see
/// [`mir::lowering::class_instantiations::admitted_this_round`]), so a program
/// that grows without end is refused after one round per level.
///
/// Returns whether the registry grew: a new instantiation has method bodies
/// still to emit, and those bodies can reach further ones.
fn register_lowered_class_instantiations(
    type_checker: &mut TypeChecker,
    bodies: &[LoweredBody],
    ast: &Program,
) -> Result<bool, CompilerError> {
    use mir::lowering::class_instantiations as instantiations;
    let found = instantiations::unregistered_instantiations(type_checker, bodies);
    if found.is_empty() {
        return Ok(false);
    }
    instantiations::refuse_past_limits(type_checker, &found, bodies)
        .map_err(CompilerError::Lowering)?;
    for admitted in instantiations::admitted_this_round(type_checker, found) {
        type_checker.record_generic_class_instantiation(&admitted.class, admitted.args);
    }
    expand_nested_generic_instantiations(type_checker, ast)?;
    Ok(true)
}

/// Register the instantiation each recorded generic class's base is reached at,
/// until a round discovers none.
///
/// An instance of `Child<String>` is also an instance of whatever `Child`
/// extends, and the methods it inherits are compiled as that class's — at that
/// class's own type arguments, which the `extends` clause maps from the child's.
/// Registering the derived instantiation is what gives those bodies an
/// instantiation to be lowered for. The loop carries the derivation up a longer
/// chain one link per round.
fn register_base_class_instantiations(type_checker: &mut TypeChecker) -> Result<(), CompilerError> {
    loop {
        let recorded: Vec<(String, Vec<Vec<Type>>)> = type_checker
            .generic_class_instantiations
            .iter()
            .map(|(name, tuples)| (name.clone(), tuples.clone()))
            .collect();
        let mut discovered: Vec<DerivedInstantiation> = Vec::new();

        for (class_name, tuples) in &recorded {
            for args in tuples {
                let base = mir::lowering::inherited_instantiation::base_class_instantiation(
                    type_checker.type_definitions(),
                    class_name,
                    args,
                );
                discovered.extend(base.map(|(name, args)| {
                    let span = args.first().map_or_else(Span::default, |arg| arg.span);
                    (name, args, span)
                }));
            }
        }

        if !record_derived_instantiations(type_checker, discovered)? {
            return Ok(());
        }
    }
}

/// The type facts the CPU backend reads, settled over `bodies`.
///
/// Drop thunks run on the host only, so a kernel body — which holds no value
/// a host thunk releases — contributes no instantiation to them.
#[cfg(feature = "cranelift")]
pub(super) fn settled_type_facts(
    pipeline_result: &PipelineResult,
    bodies: &[(String, mir::Body)],
) -> Result<mir::type_facts::TypeFacts, CompilerError> {
    mir::type_facts::TypeFacts::new(
        pipeline_result.type_checker.type_definitions().clone(),
        pipeline_result
            .type_checker
            .generic_class_instantiations
            .clone(),
        pipeline_result.vtable_fills.clone(),
        bodies
            .iter()
            .map(|(_, body)| body)
            .filter(|body| !body.is_gpu()),
    )
    .map(|facts| facts.withholding(pipeline_result.withheld_methods.clone()))
    .map_err(|refusal| {
        drop_instantiation_refusal(refusal, &pipeline_result.ast, &pipeline_result.type_checker)
    })
}

/// The error a refused drop-instantiation closure is reported as: a field
/// nesting its own type deeper on every instantiation is polymorphic
/// recursion, refused where its type is declared.
fn drop_instantiation_refusal(
    refusal: mir::type_facts::DropInstantiationRefusal,
    ast: &Program,
    type_checker: &TypeChecker,
) -> CompilerError {
    use mir::lowering::instantiation_limits as limits;
    match refusal {
        mir::type_facts::DropInstantiationRefusal::TooDeep { name, args, depth } => {
            CompilerError::Lowering(limits::polymorphic_recursion(
                &name,
                &args,
                &limits::ExceededLimit::TypeDepth(depth),
                &limits::Growth::default(),
                class_declaration_site(ast, type_checker, &name),
            ))
        }
        mir::type_facts::DropInstantiationRefusal::Unnameable { name, symbol } => {
            CompilerError::Codegen(format!(
                "two instantiations of `{name}` at types with no name would share {symbol}, \
                 laid out for only one of them"
            ))
        }
    }
}

/// What lowering a concrete class's copy of an inherited abstract-class body
/// reads: the checked program, the build mode, and the compilation-wide ids.
struct InheritedCopy<'r> {
    result: &'r PipelineResult,
    is_release: bool,
    compilation_ids: &'r mir::lowering::SharedCompilationIds,
}

/// Where the class `class_name` is declared, among the program's own
/// statements and the imported ones; the start of the file when none declares
/// it.
fn class_declaration_site(ast: &Program, type_checker: &TypeChecker, class_name: &str) -> Span {
    ast.body
        .iter()
        .chain(&type_checker.imported_statements)
        .find(|stmt| {
            matches!(&stmt.node, StatementKind::Class(class_data)
                if type_checker.type_identity_named_by(&class_data.name) == Some(class_name))
        })
        .map_or_else(Span::default, |stmt| stmt.span)
}

/// An instantiation derived from a registered one — the base it extends, the
/// type of a field — with where the declaration deriving it is written.
type DerivedInstantiation = (String, Vec<Type>, Span);

/// Record each of `discovered` the registry does not hold yet, refusing one
/// past the bounds on instantiations. Returns whether the registry grew.
fn record_derived_instantiations(
    type_checker: &mut TypeChecker,
    discovered: Vec<DerivedInstantiation>,
) -> Result<bool, CompilerError> {
    let before = recorded_instantiation_count(type_checker);
    for (name, args, span) in discovered {
        mir::lowering::class_instantiations::refuse_derived_past_limits(
            type_checker,
            &name,
            &args,
            span,
        )
        .map_err(CompilerError::Lowering)?;
        type_checker.record_generic_class_instantiation(&name, args);
    }
    Ok(recorded_instantiation_count(type_checker) != before)
}

/// How many instantiation tuples the registry holds, across every class.
fn recorded_instantiation_count(type_checker: &TypeChecker) -> usize {
    type_checker
        .generic_class_instantiations
        .values()
        .map(Vec::len)
        .sum()
}

/// Record the instantiations a program needs but never names.
///
/// A `Queue<float>` keeps its elements in a `List<T>` field, so the source only
/// ever mentions `Queue<float>` while lowering needs that inner collection's
/// methods at the concrete element width. Substitute each recorded instantiation
/// into its class's field types and record every nested generic-class type the
/// substitution makes concrete, repeating until a pass discovers nothing new so
/// a chain of nested containers is covered.
///
/// A field that nests its own class deeper on every instantiation is refused
/// at the declaration of the class it belongs to.
fn expand_nested_generic_instantiations(
    type_checker: &mut TypeChecker,
    ast: &Program,
) -> Result<(), CompilerError> {
    loop {
        let recorded: Vec<(String, Vec<Vec<Type>>)> = type_checker
            .generic_class_instantiations
            .iter()
            .map(|(name, tuples)| (name.clone(), tuples.clone()))
            .collect();
        let mut discovered: Vec<DerivedInstantiation> = Vec::new();

        for (class_name, tuples) in &recorded {
            let Some(TypeDefinition::Class(def)) =
                type_checker.type_definitions().get(class_name.as_str())
            else {
                continue;
            };
            let Some(generics) = def.generics.as_ref() else {
                continue;
            };
            let field_types: Vec<Type> = def
                .fields
                .iter()
                .map(|(_, field)| field.ty.clone())
                .collect();

            for args in tuples {
                if args.len() != generics.len() {
                    continue;
                }
                let subs: std::collections::HashMap<String, Type> = generics
                    .iter()
                    .zip(args)
                    .map(|(generic, arg)| (generic.name.clone(), arg.clone()))
                    .collect();

                for field_ty in &field_types {
                    let concrete = mir::lowering::apply_generic_sub(field_ty, &subs);
                    let TypeKind::Custom(nested_name, Some(nested_args)) = &concrete.kind else {
                        continue;
                    };
                    let mut resolved = Vec::with_capacity(nested_args.len());
                    for arg in nested_args {
                        let Ok(concrete_arg) = type_checker.extract_type_from_argument(arg) else {
                            break;
                        };
                        resolved.push(concrete_arg);
                    }
                    if resolved.len() == nested_args.len() && !resolved.is_empty() {
                        let declared = class_declaration_site(ast, type_checker, class_name);
                        discovered.push((nested_name.clone(), resolved, declared));
                    }
                }
            }
        }

        if !record_derived_instantiations(type_checker, discovered)? {
            return Ok(());
        }
    }
}

impl Pipeline {
    /// Every recorded instantiation that gets a per-instantiation body, scalar
    /// and managed alike.
    fn monomorphizable_instantiation_subs(
        result: &PipelineResult,
        class_name: &str,
    ) -> Vec<InstantiationSub> {
        let defs = result.type_checker.type_definitions();
        Self::instantiation_subs(result, class_name, &|kind| {
            mir::lowering::is_monomorphizable_type_argument(kind, defs)
        })
    }

    /// Build the substitution and mangle-argument pairs for every recorded
    /// instantiation of a generic class whose type arguments all satisfy `admits`.
    fn instantiation_subs(
        result: &PipelineResult,
        class_name: &str,
        admits: &dyn Fn(&TypeKind) -> bool,
    ) -> Vec<InstantiationSub> {
        let definition = result.type_checker.type_definitions().get(class_name);
        let Some(definition) = definition.filter(|definition| {
            matches!(
                definition,
                TypeDefinition::Class(_) | TypeDefinition::Enum(_)
            )
        }) else {
            return Vec::new();
        };
        let Some(generics) = definition.generics() else {
            return Vec::new();
        };
        let Some(instantiations) = result
            .type_checker
            .generic_class_instantiations
            .get(class_name)
        else {
            return Vec::new();
        };
        instantiations
            .iter()
            .filter(|type_args| {
                type_args.len() == generics.len() && type_args.iter().all(|t| admits(&t.kind))
            })
            .map(|type_args| {
                let pairs: Vec<(String, Type)> = generics
                    .iter()
                    .zip(type_args)
                    .map(|(g, t)| (g.name.clone(), t.clone()))
                    .collect();
                (pairs.iter().cloned().collect(), pairs)
            })
            .collect()
    }

    /// Lower one method statement for a single instantiation into a mangled body.
    ///
    /// `self` is typed at `class_subs`, the class's own arguments; the body
    /// reads what [`mir::lowering::dispatch_symbols::instantiation_substitution`]
    /// makes of them, so a trait default reads the trait's parameters at what
    /// the chain pins them to without a same-named class parameter retyping
    /// `self`.
    #[allow(clippy::too_many_arguments)]
    fn lower_one_instantiation_method(
        result: &PipelineResult,
        class_name: &str,
        method_stmt: &Statement,
        method_name: &str,
        is_release: bool,
        class_subs: &std::collections::HashMap<String, Type>,
        mangle_args: &[(String, Type)],
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let symbol = instantiated_method_symbol(class_name, method_name, mangle_args);
        if !symbols
            .claim_at(&symbol, method_stmt.span)
            .map_err(CompilerError::Lowering)?
        {
            return Ok(());
        }
        let self_type = mir::lowering::monomorphized_self_type(
            class_name,
            &result.type_checker,
            class_subs,
            method_stmt.span,
        );
        let subs = mir::lowering::dispatch_symbols::instantiation_substitution(
            &result.type_checker,
            class_name,
            method_name,
            class_subs,
        );
        let mir_body = mir::lowering::lower_class_method_at_with_compilation_ids(
            method_stmt,
            self_type,
            &result.type_checker,
            is_release,
            &subs,
            compilation_ids.clone(),
        )
        .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
        Self::push_lowered_body(bodies, symbols, symbol, mir_body)
    }

    /// Emit a generic class's monomorphized methods only where a lowered body
    /// reaches them.
    ///
    /// A collection declares dozens of methods and most call sites lower to a
    /// direct runtime call rather than the Miri body, so emitting every method
    /// for every recorded element type compiles bodies nothing can reach — and an
    /// unreachable body still has to satisfy MIR verification. The same holds for
    /// any class instantiation: `Queue<String>` inherits a `sum` default that
    /// adds its elements, which no program calls and no backend can compile, and
    /// a method of `Box<int>` nothing calls was never checked at `int`. Scan the
    /// lowered bodies for mangled symbols, emit just those, and repeat until a
    /// pass finds nothing new, since a freshly emitted body can call another.
    ///
    /// A method only a vtable slot or an element thunk names is emitted only
    /// where its obligations hold at the instance; see [`ImpliedMethodVerdicts`].
    #[allow(clippy::too_many_arguments)]
    fn lower_called_generic_class_methods(
        result: &mut PipelineResult,
        reach: &ReachTables,
        called: &mut CalledFunctions,
        verdicts: &mut ImpliedMethodVerdicts,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        loop {
            called.refresh(bodies, result, reach)?;
            let mut emitted = false;
            for wanted in Self::wanted_instance_methods(result, reach, called, symbols) {
                if wanted.demand == MethodDemand::Implied
                    && !verdicts.accepts(&mut result.type_checker, &wanted.method)
                {
                    continue;
                }
                Self::lower_wanted_method(
                    result,
                    reach,
                    &wanted,
                    is_release,
                    bodies,
                    symbols,
                    compilation_ids,
                )?;
                emitted = true;
            }
            if !emitted {
                return Ok(());
            }
        }
    }

    /// Every method a class compiles under its own name — its own and the
    /// trait defaults its clauses supply — that a lowered body reaches at one
    /// of the class's instantiations and that is not compiled yet.
    fn wanted_instance_methods(
        result: &PipelineResult,
        reach: &ReachTables,
        called: &CalledFunctions,
        symbols: &SymbolTable,
    ) -> Vec<WantedMethod> {
        Self::instance_methods(result, reach)
            .into_iter()
            .filter(|site| !symbols.is_claimed(&site.method.symbol))
            .filter_map(|site| {
                let demand = if called.is_called(&site.method.symbol) {
                    MethodDemand::Called
                } else if called.contains(&site.method.symbol)
                    || Self::is_element_method_body(
                        result,
                        &site.method.class_name,
                        &site.method.method_name,
                    )
                {
                    MethodDemand::Implied
                } else {
                    return None;
                };
                Some(WantedMethod {
                    class_site: site.class_site,
                    method_index: site.method_index,
                    method: site.method,
                    demand,
                })
            })
            .collect()
    }

    /// Every method each class and generic enum compiles under its own name,
    /// at each of its instantiations that gets a body of its own.
    fn instance_methods<'r>(
        result: &'r PipelineResult,
        reach: &ReachTables,
    ) -> Vec<InstanceMethodSite<'r>> {
        let mut sites = Vec::new();
        for (class_site, stmt) in statements_with_sites(result) {
            let Some((class_name, methods)) =
                Self::methods_compiled_per_instance(result, reach, stmt)
            else {
                continue;
            };
            for (class_subs, mangle_args) in
                Self::monomorphizable_instantiation_subs(result, class_name)
            {
                for (method_index, &(method_stmt, method_name)) in methods.iter().enumerate() {
                    sites.push(InstanceMethodSite {
                        class_site,
                        class_stmt: stmt,
                        method_index,
                        method: InstanceMethod {
                            class_name: class_name.to_string(),
                            method_name: method_name.to_string(),
                            span: method_stmt.span,
                            class_subs: class_subs.clone(),
                            symbol: instantiated_method_symbol(
                                class_name,
                                method_name,
                                &mangle_args,
                            ),
                            mangle_args: mangle_args.clone(),
                        },
                    });
                }
            }
        }
        sites
    }

    /// Lower `wanted` at its instantiation, with the generic functions it
    /// reaches.
    fn lower_wanted_method(
        result: &PipelineResult,
        reach: &ReachTables,
        wanted: &WantedMethod,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let Some(stmt) = wanted.class_site.statement(result) else {
            return Ok(());
        };
        let Some((_, methods)) = Self::methods_compiled_per_instance(result, reach, stmt) else {
            return Ok(());
        };
        let Some(&(method_stmt, _)) = methods.get(wanted.method_index) else {
            return Ok(());
        };
        let first_new = bodies.len();
        Self::lower_one_instantiation_method(
            result,
            &wanted.method.class_name,
            method_stmt,
            &wanted.method.method_name,
            is_release,
            &wanted.method.class_subs,
            &wanted.method.mangle_args,
            bodies,
            symbols,
            compilation_ids,
        )?;
        Self::lower_generic_functions_reached_from(
            result,
            is_release,
            first_new,
            reach,
            bodies,
            symbols,
            compilation_ids,
        )
    }

    /// The type `stmt` declares and the methods it compiles once per
    /// instantiation, in a stable order: a class's own methods and the trait
    /// defaults its clauses supply, or a generic enum's methods. `None` for
    /// any other statement, and for an enum that declares no parameters,
    /// whose methods are compiled once.
    fn methods_compiled_per_instance<'r>(
        result: &'r PipelineResult,
        reach: &ReachTables,
        stmt: &'r Statement,
    ) -> Option<(&'r str, Vec<(&'r Statement, &'r str)>)> {
        match &stmt.node {
            StatementKind::Class(class_data) => {
                let class_name = result
                    .type_checker
                    .type_identity_named_by(&class_data.name)?;
                let methods = Self::methods_compiled_under_class_name(
                    result,
                    &reach.trait_defaults,
                    class_data,
                    class_name,
                );
                Some((class_name, methods))
            }
            StatementKind::Enum(name_expr, Some(_), _, methods, _, _) => {
                let enum_name = result.type_checker.type_identity_named_by(name_expr)?;
                let methods = shared_method_bodies(methods)
                    .map(|(method_stmt, decl)| (method_stmt, decl.name.as_str()))
                    .collect();
                Some((enum_name, methods))
            }
            _ => None,
        }
    }

    /// Every method with a body `class_data` compiles under its own name: the
    /// ones it declares, then the trait defaults its own clauses supply.
    fn methods_compiled_under_class_name<'r>(
        result: &'r PipelineResult,
        trait_defaults: &TraitDefaultBodies,
        class_data: &'r ClassData,
        class_name: &str,
    ) -> Vec<(&'r Statement, &'r str)> {
        let definitions = result.type_checker.type_definitions();
        let defaults = mir::lowering::dispatch_symbols::own_trait_defaults(definitions, class_name)
            .into_iter()
            .filter_map(|(method, owner)| trait_defaults.method(result, owner, method));
        let declared = class_data.body.iter().chain(defaults);
        shared_method_bodies(declared.chain(Self::abstract_bodies_copied_into(result, class_name)))
            .map(|(method_stmt, decl)| (method_stmt, decl.name.as_str()))
            .collect()
    }

    /// Every body an abstract ancestor declares that the concrete class
    /// `class_name` runs, and so compiles a copy of under its own name.
    fn abstract_bodies_copied_into<'r>(
        result: &'r PipelineResult,
        class_name: &str,
    ) -> Vec<&'r Statement> {
        let definitions = result.type_checker.type_definitions();
        let is_concrete = matches!(
            definitions.get(class_name),
            Some(TypeDefinition::Class(class_def)) if !class_def.is_abstract
        );
        if !is_concrete {
            return Vec::new();
        }
        let abstract_methods = Self::abstract_class_method_bodies(result);
        crate::type_checker::context::class_ancestry(class_name, definitions)
            .skip(1)
            .flat_map(|(base_name, _)| {
                abstract_methods
                    .get(base_name)
                    .into_iter()
                    .flatten()
                    .filter(move |method_stmt| {
                        let StatementKind::FunctionDeclaration(decl) = &method_stmt.node else {
                            return false;
                        };
                        Self::runs_inherited_body(result, class_name, &decl.name, base_name)
                    })
                    .copied()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Whether this method is what a container calls on its elements.
    ///
    /// A List or Array sorts through the ordering method of a class that orders
    /// its values, a Set or Map matches elements through the `equals` a class
    /// defines and places them by the `hash` it defines beside it, and every
    /// container copies a `Cloneable` class's elements through its `clone`. Both are reached through a thunk codegen registers on the
    /// container, which is a reference no MIR body carries — so scanning call
    /// sites never finds it, and the instantiation it needs has to be emitted on
    /// the strength of the class declaring the capability.
    fn is_element_method_body(
        result: &PipelineResult,
        class_name: &str,
        method_name: &str,
    ) -> bool {
        use crate::ast::types::ORDERING_TRAIT_NAME;
        use crate::type_checker::context::{class_implements_trait, class_method_declaration};
        let definitions = result.type_checker.type_definitions();
        let [ordering, equals] = mir::lowering::dispatch_symbols::ELEMENT_METHOD_NAMES;
        // An enum's own `equals` is what `==` on it, and a set or map holding
        // it, compare through: the synthesized structural equality calls it.
        let hash = crate::ast::types::HASH_METHOD_NAME;
        if let Some(TypeDefinition::Enum(enum_def)) = definitions.get(class_name) {
            return (method_name == equals || method_name == hash)
                && enum_def.methods.contains_key(method_name);
        }
        if !matches!(definitions.get(class_name), Some(TypeDefinition::Class(_))) {
            return false;
        }
        if method_name == ordering {
            return class_implements_trait(class_name, ORDERING_TRAIT_NAME, definitions);
        }
        if method_name == crate::ast::types::CLONE_METHOD_NAME {
            return class_implements_trait(
                class_name,
                crate::ast::types::CLONEABLE_TRAIT_NAME,
                definitions,
            );
        }
        let declares = |method| class_method_declaration(class_name, method, definitions).is_some();
        (method_name == equals || method_name == hash) && declares(equals) && declares(method_name)
    }

    /// Lower every body the program reaches, each under the symbol it is
    /// linked as. A symbol whose link name another already holds refuses the
    /// program.
    pub(super) fn lower_program(
        &self,
        result: &mut PipelineResult,
        is_release: bool,
    ) -> Result<Vec<LoweredBody>, CompilerError> {
        let mut lowered = Vec::new();
        let mut symbols = SymbolTable::default();

        // One kernel-name allocator per compilation, shared across every body so
        // GPU kernel names are unique within this build and reproduced identically
        // on any later build of the same source (a long-lived host would otherwise
        // drift because the raw AST-id counter never resets).
        let compilation_ids = mir::lowering::new_shared_compilation_ids();
        let reach = ReachTables::collect(result)?;

        self.lower_program_bodies(
            result,
            is_release,
            &mut lowered,
            &mut symbols,
            &compilation_ids,
        )?;
        self.lower_imported_bodies(
            result,
            is_release,
            &mut lowered,
            &mut symbols,
            &compilation_ids,
        )?;
        self.lower_inherited_methods(
            result,
            is_release,
            &mut lowered,
            &mut symbols,
            &compilation_ids,
        )?;
        let mut verdicts = ImpliedMethodVerdicts::default();
        Self::lower_trait_default_methods(
            result,
            &reach.trait_defaults,
            &mut verdicts,
            is_release,
            &mut lowered,
            &mut symbols,
            &compilation_ids,
        )?;

        if lowered.is_empty() {
            return Err(CompilerError::Codegen(
                "No functions found to compile".to_string(),
            ));
        }

        self.lower_monomorphized_generics(
            result,
            &reach,
            is_release,
            &mut lowered,
            &mut symbols,
            &compilation_ids,
        )?;

        Self::lower_reached_methods(
            result,
            &reach,
            &mut verdicts,
            is_release,
            &mut lowered,
            &mut symbols,
            &compilation_ids,
        )?;
        Self::lower_element_equalities(result, &mut lowered, &mut symbols)?;

        Ok(lowered)
    }

    /// Lower the equality each set or map in `bodies` matches its struct,
    /// enum or tuple elements or keys through, and the hash it places every
    /// element matched through an `equals` by, once per instantiation.
    fn lower_element_equalities(
        result: &PipelineResult,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
    ) -> Result<(), CompilerError> {
        let wanted = Self::wanted_element_bodies(result, bodies)?;
        for (symbol, (element, kind)) in wanted {
            if !symbols
                .claim_at(&symbol, element.span)
                .map_err(CompilerError::Lowering)?
            {
                continue;
            }
            let lower = match kind {
                ElementBody::Equality => mir::lowering::element_equality::lower_element_equality,
                ElementBody::Hash => mir::lowering::element_equality::lower_element_hash,
            };
            let lowered = lower(&result.type_checker, &element, element.span)
                .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
            Self::push_lowered_body(bodies, symbols, symbol, lowered)?;
        }
        Ok(())
    }

    /// The equality and hash bodies the sets and maps of `bodies` match
    /// their elements or keys through, each with the element type it is
    /// lowered at, in link-name order so the bodies come out in one order
    /// whatever order the elements were found in.
    fn wanted_element_bodies(
        result: &PipelineResult,
        bodies: &[LoweredBody],
    ) -> Result<Vec<WantedElementBody>, CompilerError> {
        let type_defs = result.type_checker.type_definitions();
        let mut elements = Vec::new();
        for lowered in bodies {
            for decl in &lowered.body.local_decls {
                mir::dispatch::matched_element_types(&decl.ty, &mut elements);
            }
        }
        let mut seen = HashSet::new();
        let mut wanted = HashMap::new();
        for element in elements.iter().filter(|element| seen.insert(&element.kind)) {
            let equality = mir::dispatch::synthesized_equality_symbol(&element.kind, type_defs);
            let hash = mir::dispatch::element_hash_symbol(&element.kind, type_defs);
            let named = [(equality, ElementBody::Equality), (hash, ElementBody::Hash)];
            for (symbol, body) in named {
                if let Some(symbol) = symbol {
                    want_element_body(&mut wanted, symbol, element, body)?;
                }
            }
        }
        let mut wanted: Vec<_> = wanted.into_iter().collect();
        wanted.sort_by_cached_key(|(symbol, _)| symbol.link_name());
        Ok(wanted)
    }

    /// Emit the generic-class methods and trait defaults the lowered bodies
    /// reach, and the generic functions those reach in turn, registering each
    /// instantiation those bodies reached only through a substitution, until a
    /// round reaches nothing new.
    fn lower_reached_methods(
        result: &mut PipelineResult,
        reach: &ReachTables,
        verdicts: &mut ImpliedMethodVerdicts,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let mut called = CalledFunctions::new(reach);
        loop {
            register_base_class_instantiations(&mut result.type_checker)?;
            Self::lower_called_generic_class_methods(
                result,
                reach,
                &mut called,
                verdicts,
                is_release,
                bodies,
                symbols,
                compilation_ids,
            )?;
            let first_default = bodies.len();
            called.refresh(bodies, result, reach)?;
            Self::lower_called_trait_defaults(
                result,
                reach,
                &called,
                is_release,
                bodies,
                symbols,
                compilation_ids,
            )?;
            Self::lower_called_class_templates(
                result,
                reach,
                &called,
                is_release,
                bodies,
                symbols,
                compilation_ids,
            )?;
            Self::lower_called_shared_generic_functions(
                result,
                reach,
                &called,
                is_release,
                bodies,
                symbols,
                compilation_ids,
            )?;
            Self::lower_generic_functions_reached_from(
                result,
                is_release,
                first_default,
                reach,
                bodies,
                symbols,
                compilation_ids,
            )?;
            let registered = register_lowered_class_instantiations(
                &mut result.type_checker,
                bodies,
                &result.ast,
            )?;
            if !registered && bodies.len() == first_default {
                refuse_unnameable_generic_calls(result, reach, bodies, symbols)?;
                return Self::settle_reached_instances(result, reach, &called, verdicts, symbols);
            }
        }
    }

    /// Settle what lowering the reached methods decided: each method withheld
    /// at an instance is recorded, so codegen fills its vtable slot with the
    /// runtime trap rather than a body; the vtables are what codegen writes,
    /// and every instance compiled is answered once more.
    fn settle_reached_instances(
        result: &mut PipelineResult,
        reach: &ReachTables,
        called: &CalledFunctions,
        verdicts: &ImpliedMethodVerdicts,
        symbols: &SymbolTable,
    ) -> Result<(), CompilerError> {
        let withheld = verdicts.withheld(symbols);
        // A withheld method keeps its slot: codegen fills it with the runtime
        // trap that reports the method was not checked at this instance, so a
        // call the checker should have refused stops with a code rather than
        // jumping through a null slot.
        result.vtable_fills = called.vtables.fills();
        result.withheld_methods = withheld;
        Self::refuse_unchecked_compiled_instances(result, reach, symbols)
    }

    /// Refuse the program where a method was compiled at an instance of its
    /// class that the method's obligations do not hold at.
    ///
    /// The checker answers every obligation at each instance the program
    /// uses a method at, and lowering compiles a method only where a lowered
    /// body reaches it, so this finds nothing unless the two disagree about
    /// what a program reaches. Where they do, the body would otherwise run on
    /// a type nobody checked it against; the refusal the checker would have
    /// given is reported instead.
    fn refuse_unchecked_compiled_instances(
        result: &mut PipelineResult,
        reach: &ReachTables,
        symbols: &SymbolTable,
    ) -> Result<(), CompilerError> {
        let compiled = Self::compiled_class_instances(result, reach, symbols);
        let refusals: Vec<_> = compiled
            .iter()
            .flat_map(|instance| result.type_checker.compiled_instance_refusals(instance))
            .collect();
        if refusals.is_empty() {
            return Ok(());
        }
        Err(CompilerError::TypeErrors {
            errors: refusals,
            warnings: result.type_checker.warnings().to_vec(),
        })
    }

    /// Every method lowering compiled at an instantiation of its generic
    /// class, as the symbol table records it.
    fn compiled_class_instances(
        result: &PipelineResult,
        reach: &ReachTables,
        symbols: &SymbolTable,
    ) -> Vec<CompiledInstance> {
        Self::instance_methods(result, reach)
            .into_iter()
            .filter(|site| matches!(site.class_stmt.node, StatementKind::Class(_)))
            .filter(|site| symbols.is_claimed(&site.method.symbol))
            .map(|site| {
                let receiver = mir::lowering::monomorphized_self_type(
                    &site.method.class_name,
                    &result.type_checker,
                    &site.method.class_subs,
                    site.class_stmt.span,
                );
                site.method.compiled_at(receiver)
            })
            .collect()
    }

    /// Emit the shared `{Trait}_{method}` body of each trait default a lowered
    /// body calls or codegen names outside any call.
    ///
    /// The shared body is written in the trait's own parameters and leaves them
    /// bare, so a local at `T` is a value of no known type. A concrete
    /// implementor has its own copy typed at what its clauses pin, which its
    /// static calls and its vtable slots name; a generic implementor's vtable
    /// names the copy compiled for the instantiation it was built at. What
    /// still names the shared body — the bare vtable of a generic implementor
    /// built at arguments with no spelling, a caller whose `self` is abstract,
    /// an abstract class's drop hook — is found here; one nothing names is
    /// never compiled.
    #[allow(clippy::too_many_arguments)]
    fn lower_called_trait_defaults(
        result: &PipelineResult,
        reach: &ReachTables,
        called: &CalledFunctions,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        for default in &reach.trait_defaults.bodies {
            let symbol = &default.symbol;
            if symbols.is_claimed(symbol) {
                continue;
            }
            let is_named = called.contains(symbol) || reach.synthesized.contains(symbol);
            if !is_named {
                continue;
            }
            let Some((trait_stmt, method_stmt)) = default.statements(result) else {
                continue;
            };
            if !symbols
                .claim_at(symbol, method_stmt.span)
                .map_err(CompilerError::Lowering)?
            {
                continue;
            }
            let self_type = Type::new(
                TypeKind::Custom(default.trait_name.clone(), None),
                trait_stmt.span,
            );
            let body = mir::lowering::lower_class_method_with_compilation_ids(
                method_stmt,
                self_type,
                &result.type_checker,
                is_release,
                compilation_ids.clone(),
            )
            .map_err(|e| monomorphized_lowering_failure(symbol, e))?;
            Self::push_lowered_body(bodies, symbols, symbol.clone(), body)?;
        }
        Ok(())
    }

    /// Emit the open template of each generic class method a lowered body
    /// names: a call, or a slot of a vtable built where the class's arguments
    /// have no spelling. One nothing names is never compiled.
    fn lower_called_class_templates(
        result: &PipelineResult,
        reach: &ReachTables,
        called: &CalledFunctions,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        for template in &reach.class_templates {
            let symbol = &template.symbol;
            let is_named = called.contains(symbol) || reach.synthesized.contains(symbol);
            if !is_named || symbols.is_claimed(symbol) {
                continue;
            }
            let Some((class_stmt, method_stmt)) = template.statements(result) else {
                continue;
            };
            if !symbols
                .claim_at(symbol, method_stmt.span)
                .map_err(CompilerError::Lowering)?
            {
                continue;
            }
            let self_type = Type::new(
                TypeKind::Custom(template.class_name.clone(), None),
                class_stmt.span,
            );
            let body = mir::lowering::lower_class_method_with_compilation_ids(
                method_stmt,
                self_type,
                &result.type_checker,
                is_release,
                compilation_ids.clone(),
            )
            .map_err(|e| monomorphized_lowering_failure(symbol, e))?;
            Self::push_lowered_body(bodies, symbols, symbol.clone(), body)?;
        }
        Ok(())
    }

    /// Emit the shared body of each generic function a lowered body calls by
    /// its bare symbol.
    ///
    /// The shared body is written in the function's own parameters and leaves
    /// them open, so it cannot know whether an element it reads is reference
    /// counted. A call at concrete arguments names the copy compiled for them;
    /// only a body that is itself still open — the shared body of a generic
    /// caller handing its own parameters on — names the shared one. So one
    /// nothing names is never compiled, and never reaches a collection method
    /// at an element type it cannot own correctly.
    // TODO: only calls are scanned for a bare symbol. A generic function bound
    // to a local (`let g = ident`) is not a reference to its shared body today —
    // it fails to link as `g` at its argument — and making such a value work has
    // to name the per-instantiation body, never the shared one this scan finds.
    #[allow(clippy::too_many_arguments)]
    fn lower_called_shared_generic_functions(
        result: &PipelineResult,
        reach: &ReachTables,
        called: &CalledFunctions,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let mut named: Vec<(Symbol, &DeclaredFunction, StatementSite)> = reach
            .generic_functions
            .iter()
            .filter_map(|(function, site)| {
                let symbol = Symbol::declared_function(&function.module, &function.name);
                let is_named = called.contains(&symbol) || reach.synthesized.contains(&symbol);
                (is_named && !symbols.is_claimed(&symbol)).then_some((symbol, function, *site))
            })
            .collect();
        // Emitted in link-name order, so the bodies come out in one order
        // whatever order the declarations were found in.
        named.sort_by_cached_key(|(symbol, _, _)| symbol.link_name());
        for (_, function, site) in named {
            let Some(stmt) = site.statement(result) else {
                continue;
            };
            Self::lower_top_level_function(
                result,
                stmt,
                &function.name,
                is_release,
                bodies,
                symbols,
                compilation_ids,
            )?;
        }
        Ok(())
    }

    /// Lower top-level functions and the methods of every class/struct/enum
    /// declared in the user's program AST. A trait's default bodies are emitted
    /// only where something calls them, by `lower_called_trait_defaults`, and a
    /// generic class's methods only where something names them, by
    /// `lower_called_class_templates`.
    fn lower_program_bodies(
        &self,
        result: &PipelineResult,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        // Lower functions and class methods from the program AST
        for stmt in &result.ast.body {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) if decl.generics.is_some() => {}
                StatementKind::FunctionDeclaration(decl) => {
                    Self::lower_top_level_function(
                        result,
                        stmt,
                        &decl.name,
                        is_release,
                        bodies,
                        symbols,
                        compilation_ids,
                    )?;
                }
                // A generic class's methods are its open templates, compiled
                // only where something names one, by `lower_called_class_templates`.
                StatementKind::Class(class_data) if class_data.generics.is_some() => {}
                StatementKind::Class(class_data) => {
                    let Some(class_name) =
                        result.type_checker.type_identity_named_by(&class_data.name)
                    else {
                        continue;
                    };

                    let self_type =
                        Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);

                    for (method_stmt, method_decl) in shared_method_bodies(&class_data.body) {
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

                        let symbol = Symbol::method(class_name, &[], &method_decl.name, &[]);
                        if !symbols
                            .claim_at(&symbol, method_stmt.span)
                            .map_err(CompilerError::Lowering)?
                        {
                            continue;
                        }

                        let mir_body = mir::lowering::lower_class_method_with_compilation_ids(
                            method_stmt,
                            self_type.clone(),
                            &result.type_checker,
                            is_release,
                            compilation_ids.clone(),
                        )
                        .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
                        Self::push_lowered_body(bodies, symbols, symbol, mir_body)?;
                    }
                }
                // A struct holds data only and declares no methods to lower.
                StatementKind::Struct(..) => {}
                StatementKind::Enum(
                    name_expr,
                    _generics,
                    _variants,
                    methods,
                    _vis,
                    _attributes,
                ) => {
                    // Compile enum methods as `EnumName_methodName` using lower_class_method.
                    // Enum methods have no explicit `self` in their param list (like class methods).
                    let Some(enum_name) = result.type_checker.type_identity_named_by(name_expr)
                    else {
                        continue;
                    };

                    let self_type = crate::ast::types::Type::new(
                        crate::ast::types::TypeKind::Custom(enum_name.to_string(), None),
                        stmt.span,
                    );

                    for (method_stmt, method_decl) in shared_method_bodies(methods) {
                        let symbol = Symbol::method(enum_name, &[], &method_decl.name, &[]);
                        if !symbols
                            .claim_at(&symbol, method_stmt.span)
                            .map_err(CompilerError::Lowering)?
                        {
                            continue;
                        }

                        let mir_body = mir::lowering::lower_class_method_with_compilation_ids(
                            method_stmt,
                            self_type.clone(),
                            &result.type_checker,
                            is_release,
                            compilation_ids.clone(),
                        )
                        .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
                        Self::push_lowered_body(bodies, symbols, symbol, mir_body)?;
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
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        // Lower functions and class methods imported from stdlib modules
        for stmt in &result.type_checker.imported_statements {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) if decl.generics.is_some() => {}
                StatementKind::FunctionDeclaration(decl) => {
                    Self::lower_top_level_function(
                        result,
                        stmt,
                        &decl.name,
                        is_release,
                        bodies,
                        symbols,
                        compilation_ids,
                    )?;
                }
                // A generic class's methods are its open templates, compiled
                // only where something names one, by `lower_called_class_templates`.
                StatementKind::Class(class_data) if class_data.generics.is_some() => {}
                StatementKind::Class(class_data) => {
                    // Extract the class name string
                    let Some(class_name) =
                        result.type_checker.type_identity_named_by(&class_data.name)
                    else {
                        continue;
                    };

                    // Build the `self` type for this class
                    let self_type =
                        Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);

                    // Compile each non-runtime method in the class body
                    for (method_stmt, method_decl) in shared_method_bodies(&class_data.body) {
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

                        let symbol = Symbol::method(class_name, &[], &method_decl.name, &[]);
                        if !symbols
                            .claim_at(&symbol, method_stmt.span)
                            .map_err(CompilerError::Lowering)?
                        {
                            continue;
                        }

                        let mir_body = mir::lowering::lower_class_method_with_compilation_ids(
                            method_stmt,
                            self_type.clone(),
                            &result.type_checker,
                            is_release,
                            compilation_ids.clone(),
                        )
                        .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
                        Self::push_lowered_body(bodies, symbols, symbol, mir_body)?;
                    }
                }
                // A struct holds data only and declares no methods to lower.
                StatementKind::Struct(..) => {}
                StatementKind::Enum(
                    name_expr,
                    _generics,
                    _variants,
                    methods,
                    _vis,
                    _attributes,
                ) => {
                    let Some(enum_name) = result.type_checker.type_identity_named_by(name_expr)
                    else {
                        continue;
                    };

                    let self_type =
                        Type::new(TypeKind::Custom(enum_name.to_string(), None), stmt.span);

                    for (method_stmt, method_decl) in shared_method_bodies(methods) {
                        let symbol = Symbol::method(enum_name, &[], &method_decl.name, &[]);
                        if !symbols
                            .claim_at(&symbol, method_stmt.span)
                            .map_err(CompilerError::Lowering)?
                        {
                            continue;
                        }

                        let mir_body = mir::lowering::lower_class_method_with_compilation_ids(
                            method_stmt,
                            self_type.clone(),
                            &result.type_checker,
                            is_release,
                            compilation_ids.clone(),
                        )
                        .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
                        Self::push_lowered_body(bodies, symbols, symbol, mir_body)?;
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
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let abstract_methods = Self::abstract_class_method_bodies(result);
        let copy = InheritedCopy {
            result,
            is_release,
            compilation_ids,
        };
        for stmt in Self::class_statements(result) {
            let StatementKind::Class(class_data) = &stmt.node else {
                continue;
            };
            let Some(class_name) = result.type_checker.type_identity_named_by(&class_data.name)
            else {
                continue;
            };
            let is_concrete = matches!(
                result.type_checker.type_definitions().get(class_name),
                Some(TypeDefinition::Class(cd)) if !cd.is_abstract
            );
            if is_concrete {
                let self_type =
                    Type::new(TypeKind::Custom(class_name.to_string(), None), stmt.span);
                Self::lower_inherited_abstract_bodies(
                    &copy,
                    class_name,
                    &self_type,
                    &abstract_methods,
                    bodies,
                    symbols,
                )?;
            }
        }
        Ok(())
    }

    /// Every statement of the program and of the modules it imports.
    fn class_statements(result: &PipelineResult) -> impl Iterator<Item = &Statement> {
        result
            .ast
            .body
            .iter()
            .chain(result.type_checker.imported_statements.iter())
    }

    /// Each abstract class's methods that carry a body, by class name.
    fn abstract_class_method_bodies(
        result: &PipelineResult,
    ) -> std::collections::HashMap<String, Vec<&Statement>> {
        let mut methods: std::collections::HashMap<String, Vec<&Statement>> =
            std::collections::HashMap::new();
        for stmt in Self::class_statements(result) {
            let StatementKind::Class(class_data) = &stmt.node else {
                continue;
            };
            let Some(class_name) = result.type_checker.type_identity_named_by(&class_data.name)
            else {
                continue;
            };
            let is_abstract = matches!(
                result.type_checker.type_definitions().get(class_name),
                Some(TypeDefinition::Class(cd)) if cd.is_abstract
            );
            if !is_abstract {
                continue;
            }
            let with_bodies = class_data.body.iter().filter(|method_stmt| {
                matches!(&method_stmt.node, StatementKind::FunctionDeclaration(md) if md.body.is_some())
            });
            methods
                .entry(class_name.to_string())
                .or_default()
                .extend(with_bodies);
        }
        methods
    }

    /// Lower the concrete class `class_name`'s own copy of every body an
    /// abstract ancestor declares and the class runs.
    ///
    /// Every abstract ancestor counts, past any concrete class between: static
    /// dispatch names this class's own copy of whatever abstract-class body it
    /// runs, however far up the chain that body is declared.
    fn lower_inherited_abstract_bodies(
        copy: &InheritedCopy,
        class_name: &str,
        self_type: &Type,
        abstract_methods: &std::collections::HashMap<String, Vec<&Statement>>,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
    ) -> Result<(), CompilerError> {
        let result = copy.result;
        let definitions = result.type_checker.type_definitions();
        // Each copy reads the ancestor's parameters at what the `extends`
        // chain pins them to: `T` of `Base<T>` under `extends Base<int>` is
        // `int` in the copy, as the trait-default copies read their pins.
        let supertypes = result.type_checker.declaring_types_above(
            class_name,
            &mir::lowering::inherited_instantiation::own_parameters_left_open(
                definitions,
                class_name,
            ),
        );
        let unpinned = std::collections::HashMap::new();
        let ancestors =
            crate::type_checker::context::class_ancestry(class_name, definitions).skip(1);
        for (base_name, _) in ancestors {
            let pinned = pins_of(&supertypes, base_name).unwrap_or(&unpinned);
            for method_stmt in abstract_methods.get(base_name).into_iter().flatten() {
                let Some(md) = shared_method_body(method_stmt) else {
                    continue;
                };
                if !Self::runs_inherited_body(result, class_name, &md.name, base_name) {
                    continue;
                }
                let symbol = Symbol::method(class_name, &[], &md.name, &[]);
                if !symbols
                    .claim_at(&symbol, method_stmt.span)
                    .map_err(CompilerError::Lowering)?
                {
                    continue;
                }
                let mir_body = mir::lowering::lower_class_method_at_with_compilation_ids(
                    method_stmt,
                    self_type.clone(),
                    &result.type_checker,
                    copy.is_release,
                    pinned,
                    copy.compilation_ids.clone(),
                )
                .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
                Self::push_lowered_body(bodies, symbols, symbol, mir_body)?;
            }
        }
        Ok(())
    }

    /// Whether a `class_name` receiver runs the body `ancestor` declares for
    /// `method_name` — not one the class overrides, one a nearer class
    /// replaces, or one re-declared abstract below `ancestor`.
    fn runs_inherited_body(
        result: &PipelineResult,
        class_name: &str,
        method_name: &str,
        ancestor: &str,
    ) -> bool {
        matches!(
            crate::type_checker::context::resolve_method_source(
                result.type_checker.type_definitions(),
                class_name,
                method_name,
            ),
            Some(crate::type_checker::context::MethodSource::Declared { class, .. })
                if class == ancestor
        )
    }

    /// Re-lower trait default methods per concrete class.
    ///
    /// Mirrors `lower_inherited_methods` for traits. When a concrete class C
    /// inherits a default method M, we synthesize `C_M` by re-lowering the
    /// default's body with self_type = C. This lets `resolve_inherited_method`
    /// return C as the defining class (concrete-caller rule), so calls dispatch
    /// statically to `C_M`. Inside C_M, self-calls like `self.length()` also
    /// dispatch statically because self_type is concrete — avoiding the
    /// slot-mismatch issue between per-trait and combined-vtable method layouts.
    /// Every re-lowering emits the default body's closures again; they are kept
    /// apart because `LoweringContext::closure_symbol` spells the implementing
    /// class after the closure's own base name.
    ///
    /// A class with no parameters of its own reads the default at what its
    /// clauses pin, which is an instance of the default like any other: one
    /// whose obligations fail there is withheld, as [`ImpliedMethodVerdicts`]
    /// states. A call written to it was refused by the checker already.
    fn lower_trait_default_methods(
        result: &mut PipelineResult,
        trait_defaults: &TraitDefaultBodies,
        verdicts: &mut ImpliedMethodVerdicts,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        for class_name in Self::classes_inheriting_defaults(result) {
            let accepted =
                Self::judged_trait_default_copies(result, trait_defaults, verdicts, &class_name);
            Self::lower_class_trait_defaults(
                result,
                &class_name,
                &accepted,
                trait_defaults,
                is_release,
                bodies,
                symbols,
                compilation_ids,
            )?;
        }
        Ok(())
    }

    /// Every concrete class the program declares, by name.
    fn classes_inheriting_defaults(result: &PipelineResult) -> Vec<String> {
        let definitions = result.type_checker.type_definitions();
        statements_with_sites(result)
            .filter_map(|(_, stmt)| {
                let StatementKind::Class(class_data) = &stmt.node else {
                    return None;
                };
                let class_name = result
                    .type_checker
                    .type_identity_named_by(&class_data.name)?;
                let Some(TypeDefinition::Class(class_def)) = definitions.get(class_name) else {
                    return None;
                };
                (!class_def.is_abstract).then(|| class_name.to_string())
            })
            .collect()
    }

    /// The trait defaults `class_name` inherits whose copy is compiled, each
    /// with the trait supplying it: every one for a generic class, whose
    /// copy leaves its own parameters open, and for any other class each one
    /// whose obligations hold at what its clauses pin.
    fn judged_trait_default_copies(
        result: &mut PipelineResult,
        trait_defaults: &TraitDefaultBodies,
        verdicts: &mut ImpliedMethodVerdicts,
        class_name: &str,
    ) -> Vec<(String, String)> {
        let definitions = result.type_checker.type_definitions();
        let is_generic = matches!(
            definitions.get(class_name),
            Some(TypeDefinition::Class(class_def)) if class_def.generics.is_some()
        );
        let inherited: Vec<(String, String, Span)> =
            mir::lowering::dispatch_symbols::inherited_trait_defaults(definitions, class_name)
                .into_iter()
                .filter_map(|(method, owner)| {
                    let span = trait_defaults.method(result, owner, method)?.span;
                    Some((method.to_string(), owner.to_string(), span))
                })
                .collect();
        inherited
            .into_iter()
            .filter(|(method, _, span)| {
                is_generic
                    || verdicts.accepts_class_copy(
                        &mut result.type_checker,
                        class_name,
                        method,
                        *span,
                    )
            })
            .map(|(method, owner, _)| (method, owner))
            .collect()
    }

    /// Lower `class_name`'s copy of each trait default in `inherited`, typed
    /// at what its `extends` and `implements` clauses pin the trait's own
    /// parameters to.
    ///
    /// A parameter only the class's own type arguments would pin is absent
    /// from the pins: the class's bare copy leaves it generic, and each
    /// instantiation's copy reads it from that instantiation.
    #[allow(clippy::too_many_arguments)]
    fn lower_class_trait_defaults(
        result: &PipelineResult,
        class_name: &str,
        inherited: &[(String, String)],
        trait_defaults: &TraitDefaultBodies,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let definitions = result.type_checker.type_definitions();
        let self_type = Type::new(
            TypeKind::Custom(class_name.to_string(), None),
            class_declaration_site(&result.ast, &result.type_checker, class_name),
        );
        let supertypes = result.type_checker.declaring_types_above(
            class_name,
            &mir::lowering::inherited_instantiation::own_parameters_left_open(
                definitions,
                class_name,
            ),
        );
        let unpinned = std::collections::HashMap::new();
        for (method_name, trait_name) in inherited {
            let Some(method_stmt) = trait_defaults.method(result, trait_name, method_name) else {
                continue;
            };
            if shared_method_body(method_stmt).is_none() {
                continue;
            }
            let symbol = Symbol::method(class_name, &[], method_name, &[]);
            if !symbols
                .claim_at(&symbol, method_stmt.span)
                .map_err(CompilerError::Lowering)?
            {
                continue;
            }
            let pinned = pins_of(&supertypes, trait_name).unwrap_or(&unpinned);
            let mir_body = mir::lowering::lower_class_method_at_with_compilation_ids(
                method_stmt,
                self_type.clone(),
                &result.type_checker,
                is_release,
                pinned,
                compilation_ids.clone(),
            )
            .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
            Self::push_lowered_body(bodies, symbols, symbol, mir_body)?;
        }
        Ok(())
    }

    /// Monomorphize generic functions: collect every generic function call the
    /// already-lowered bodies recorded, then re-lower the original generic for
    /// each unique instantiation.
    ///
    /// An instantiation's body is scanned in turn, because a generic body that
    /// calls another generic body names that callee at the type it was itself
    /// instantiated at — an instantiation no call written in the source spells.
    /// The worklist runs until no body calls a missing one.
    fn lower_monomorphized_generics(
        &self,
        result: &PipelineResult,
        reach: &ReachTables,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        // Residency specialization is scanned over the bodies lowered so far
        // only: a specialized body cannot itself reach another specialization,
        // because forwarding a buffer parameter to a further call is
        // buffer-touching and the type checker rejects it.
        let needed_residency = Self::residency_specializations_called(bodies, symbols);

        Self::lower_generic_functions_reached_from(
            result,
            is_release,
            0,
            reach,
            bodies,
            symbols,
            compilation_ids,
        )?;

        self.lower_residency_specializations(
            result,
            is_release,
            needed_residency,
            bodies,
            symbols,
            compilation_ids,
        )
    }

    /// Lower the top-level function `name`, declared by `stmt`, under the
    /// symbol of the module that declares it, unless `stmt`'s body already
    /// holds that symbol. Another declaration holding it is refused.
    #[allow(clippy::too_many_arguments)]
    fn lower_top_level_function(
        result: &PipelineResult,
        stmt: &Statement,
        name: &str,
        is_release: bool,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let module = declaring_module(result, stmt, name)?;
        let symbol = Symbol::declared_function(module, name);
        if !symbols
            .claim_definition(&symbol, stmt.id, stmt.span)
            .map_err(CompilerError::Lowering)?
        {
            return Ok(());
        }
        let body = mir::lowering::lower_function_with_compilation_ids(
            stmt,
            &result.type_checker,
            is_release,
            true,
            compilation_ids.clone(),
        )
        .map_err(|e| monomorphized_lowering_failure(&symbol, e))?;
        Self::push_lowered_body(bodies, symbols, symbol, body)
    }

    /// Record a lowered body under `symbol`, which the caller has claimed,
    /// and each lambda lowered with it under the symbol it claims now.
    pub(super) fn push_lowered_body(
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        symbol: Symbol,
        lowered: mir::lowering::Lowered,
    ) -> Result<(), CompilerError> {
        bodies.push(LoweredBody {
            symbol,
            body: lowered.body,
            requests: lowered.requests,
        });
        for lambda in lowered.lambdas {
            if symbols
                .claim_at(&lambda.symbol, lambda.body.span)
                .map_err(CompilerError::Lowering)?
            {
                bodies.push(LoweredBody {
                    symbol: lambda.symbol,
                    body: lambda.body,
                    requests: lambda.requests,
                });
            }
        }
        Ok(())
    }

    /// The residency-specialized calls in `bodies` that have no body yet, one
    /// per symbol, ordered by link name, each with the function it specializes
    /// and the per-argument device handles, as the call's lowering recorded
    /// them.
    fn residency_specializations_called(
        bodies: &[LoweredBody],
        symbols: &SymbolTable,
    ) -> Vec<mir::lowering::requests::ResidencyFunctionCall> {
        let mut needed: Vec<mir::lowering::requests::ResidencyFunctionCall> = bodies
            .iter()
            .flat_map(|lowered| lowered.requests.residency_function_calls.iter())
            .filter(|call| !symbols.is_claimed(&call.symbol))
            .cloned()
            .collect();
        needed.sort_by_cached_key(|call| call.symbol.link_name());
        needed.dedup_by(|later, earlier| later.symbol == earlier.symbol);
        needed
    }

    /// Lower each needed residency specialization. The parameter that received
    /// a gpu-resident buffer is stamped with its device handle so the
    /// specialized body's `forall` launches on that buffer.
    fn lower_residency_specializations(
        &self,
        result: &PipelineResult,
        is_release: bool,
        needed: Vec<mir::lowering::requests::ResidencyFunctionCall>,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        // Residency specialization applies to any function (generic or not),
        // so it needs every function declaration, not only generic ones.
        let decls = function_declarations(result, |_| true)?;
        for call in needed {
            let Some(ast_stmt) = decls
                .get(&call.function)
                .and_then(|site| site.statement(result))
            else {
                continue;
            };
            if !symbols
                .claim_at(&call.symbol, ast_stmt.span)
                .map_err(CompilerError::Lowering)?
            {
                continue;
            }
            let subs: std::collections::HashMap<String, Type> =
                call.type_args.iter().cloned().collect();
            let body = mir::lowering::lower_residency_instantiation_with_compilation_ids(
                ast_stmt,
                &result.type_checker,
                is_release,
                true,
                &subs,
                &call.arg_handles,
                compilation_ids.clone(),
            )
            .map_err(|e| monomorphized_lowering_failure(&call.symbol, e))?;
            Self::push_lowered_body(bodies, symbols, call.symbol, body)?;
        }
        Ok(())
    }
}

/// A body a set or map matches its elements through: the symbol it is
/// lowered under, the element type it is lowered at, and which body it is.
type WantedElementBody = (Symbol, (Type, ElementBody));

/// Record that `symbol` names the `body` lowered at `element`, refusing a
/// symbol another element type already holds: the one body lowered under it
/// would compare or hash the other type's values by the wrong layout.
fn want_element_body(
    wanted: &mut HashMap<Symbol, (Type, ElementBody)>,
    symbol: Symbol,
    element: &Type,
    body: ElementBody,
) -> Result<(), CompilerError> {
    match wanted.entry(symbol) {
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert((element.clone(), body));
            Ok(())
        }
        std::collections::hash_map::Entry::Occupied(held) => {
            let (first, _) = held.get();
            if first.kind.to_string() == element.kind.to_string() {
                return Ok(());
            }
            Err(CompilerError::Internal(format!(
                "the element types `{}` and `{}` both name `{}`",
                first.kind,
                element.kind,
                held.key().link_name()
            )))
        }
    }
}

/// Which body a set or map asks of its elements a pipeline pass lowers.
#[derive(Clone, Copy)]
enum ElementBody {
    /// The equality synthesized from a struct's or enum's `==`.
    Equality,
    /// The hash consistent with an element's equality.
    Hash,
}

#[cfg(test)]
mod lowering_failure_tests {
    use super::monomorphized_lowering_failure;
    use crate::error::lowering::LoweringError;
    use crate::error::syntax::Span;
    use crate::mir::symbol::Symbol;

    /// A lowering failure of the compiler's own is reported where lowering
    /// stopped: the internal-error code, and the span of the construct, so an
    /// author has a line to look at rather than nothing.
    #[test]
    fn an_internal_lowering_failure_carries_its_location() {
        let span = Span::new(40, 52);
        let error = LoweringError::unsupported_expression("a construct lowering refuses", span);
        let diagnostics = monomorphized_lowering_failure(&Symbol::entry(), error).to_diagnostics();
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code, Some("MER_CG_008"));
        assert_eq!(diagnostic.span, Some(span));
        assert!(diagnostic.message.contains("a construct lowering refuses"));
    }
}
