// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The worklist lowering every generic function and generic method copy the
//! lowered bodies call, and every one those copies call in turn.
//!
//! Each lowered body records the instantiations it calls, with the calling
//! body's substitution already applied. The worklist claims each symbol not
//! yet compiled, lowers its copy, and scans the new body for further calls,
//! until no body calls a missing one.

use super::lowering::{
    monomorphized_lowering_failure, statements_with_sites, ReachTables, StatementSite,
};
use super::*;
use crate::mir::lowering::requests::{GenericFunctionCall, GenericMethodCall};

/// Where a lowered instantiation goes: the bodies lowered so far and the
/// symbols they claim, lowered for a release build or not, with kernel names
/// drawn from the compilation's allocator.
struct LoweringSink<'s> {
    is_release: bool,
    bodies: &'s mut Vec<LoweredBody>,
    symbols: &'s mut SymbolTable,
    compilation_ids: &'s mir::lowering::SharedCompilationIds,
}

/// Every method a class or an enum of the program or its imports declares
/// type parameters of its own for (`fn m<T>`) and gives a body, by the
/// declaring type and the method's name. The program's own declaration of a
/// name comes first and wins.
pub(super) struct GenericMethodBodies {
    by_method: std::collections::HashMap<(String, String), (StatementSite, usize)>,
}

impl GenericMethodBodies {
    pub(super) fn collect(result: &PipelineResult) -> Self {
        let mut by_method = std::collections::HashMap::new();
        for (site, stmt) in statements_with_sites(result) {
            let Some((name_expr, methods)) = Self::declared_methods(stmt) else {
                continue;
            };
            let Some(type_name) = result.type_checker.type_identity_named_by(name_expr) else {
                continue;
            };
            for (member, method_stmt) in methods.iter().enumerate() {
                let StatementKind::FunctionDeclaration(decl) = &method_stmt.node else {
                    continue;
                };
                if decl.body.is_some() && decl.declares_type_parameters() {
                    by_method
                        .entry((type_name.to_string(), decl.name.clone()))
                        .or_insert((site, member));
                }
            }
        }
        GenericMethodBodies { by_method }
    }

    /// The statement of the generic method `method` the class or enum
    /// `type_name` declares itself.
    fn declared<'r>(
        &self,
        result: &'r PipelineResult,
        type_name: &str,
        method: &str,
    ) -> Option<&'r Statement> {
        let &(site, member) = self
            .by_method
            .get(&(type_name.to_string(), method.to_string()))?;
        let (_, methods) = Self::declared_methods(site.statement(result)?)?;
        methods.get(member)
    }

    /// The name and the method statements of the class or enum `stmt`
    /// declares; `None` for any other statement.
    fn declared_methods(stmt: &Statement) -> Option<(&crate::ast::Expression, &[Statement])> {
        if let StatementKind::Class(class_data) = &stmt.node {
            return Some((&class_data.name, &class_data.body));
        }
        if let StatementKind::Enum(name_expr, _, _, methods, _, _) = &stmt.node {
            return Some((name_expr, methods));
        }
        None
    }
}

/// The statement whose body the copy of the generic method `call` names
/// runs: the method of the nearest class in the owner's chain that gives it
/// a body, else the trait default the chain inherits; an enum's own method.
fn generic_method_statement<'r>(
    result: &'r PipelineResult,
    reach: &ReachTables,
    call: &GenericMethodCall,
) -> Option<&'r Statement> {
    use crate::type_checker::context::{resolve_method_source, MethodSource};
    let definitions = result.type_checker.type_definitions();
    let method = call.method.as_str();
    match definitions.get(&call.owner)? {
        TypeDefinition::Class(_) => {
            match resolve_method_source(definitions, &call.owner, method)? {
                MethodSource::Declared { class, .. } => {
                    reach.generic_methods.declared(result, class, method)
                }
                MethodSource::Default(default) => {
                    reach
                        .trait_defaults
                        .method(result, default.trait_name, method)
                }
                MethodSource::AbstractOnly { .. } => None,
            }
        }
        TypeDefinition::Enum(_) => reach.generic_methods.declared(result, &call.owner, method),
        TypeDefinition::Trait(_)
        | TypeDefinition::Struct(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_) => None,
    }
}

/// A generic function or method instantiation waiting in the worklist.
enum GenericInstantiation {
    Function(GenericFunctionCall),
    Method(GenericMethodCall),
}

impl GenericInstantiation {
    fn symbol(&self) -> &Symbol {
        match self {
            GenericInstantiation::Function(call) => &call.symbol,
            GenericInstantiation::Method(call) => &call.symbol,
        }
    }
}

/// A generic function or method instantiation a lowered body calls, as the
/// body recorded it.
#[derive(Clone, Copy)]
enum CalledInstantiation<'c> {
    Function(&'c GenericFunctionCall),
    Method(&'c GenericMethodCall),
}

impl<'c> CalledInstantiation<'c> {
    fn symbol(self) -> &'c Symbol {
        match self {
            CalledInstantiation::Function(call) => &call.symbol,
            CalledInstantiation::Method(call) => &call.symbol,
        }
    }

    /// The instantiation as the worklist holds it.
    fn queued(self) -> GenericInstantiation {
        match self {
            CalledInstantiation::Function(call) => GenericInstantiation::Function(call.clone()),
            CalledInstantiation::Method(call) => GenericInstantiation::Method(call.clone()),
        }
    }
}

/// Every generic function and method instantiation `lowered` calls: the
/// functions, then the methods, each in call order.
fn generic_instantiations_called(
    lowered: &LoweredBody,
) -> impl Iterator<Item = CalledInstantiation<'_>> {
    let functions =
        (lowered.requests.generic_function_calls.iter()).map(CalledInstantiation::Function);
    let methods = (lowered.requests.generic_method_calls.iter()).map(CalledInstantiation::Method);
    functions.chain(methods)
}

/// Refuse the first generic function instantiation a lowered body calls at a
/// type argument with no name, which the worklist leaves unlowered rather than
/// share one body between every such call.
///
/// A body lowered for one instantiation reaches such a call when it calls its
/// own function at an ever deeper type; the class instances it builds on the
/// way are refused first, with the chain that grew them, by the bounds on
/// instantiations. When none is, the call is refused here: as polymorphic
/// recursion when its argument nests past the depth one instance may nest to,
/// otherwise as an argument with no name.
pub(super) fn refuse_unnameable_generic_calls(
    result: &PipelineResult,
    reach: &ReachTables,
    bodies: &[LoweredBody],
    symbols: &SymbolTable,
) -> Result<(), CompilerError> {
    use mir::lowering::instantiation_limits as limits;
    let Some(call) = bodies
        .iter()
        .flat_map(generic_instantiations_called)
        .find(|call| {
            !symbols.is_claimed(call.symbol()) && call.symbol().has_an_unnameable_argument()
        })
    else {
        return Ok(());
    };
    let (name, type_args, declaration) = match call {
        CalledInstantiation::Function(call) => (
            &call.function.name,
            &call.type_args,
            reach
                .generic_functions
                .get(&call.function)
                .and_then(|site| site.statement(result)),
        ),
        CalledInstantiation::Method(call) => (
            &call.method,
            &call.type_args,
            generic_method_statement(result, reach, call),
        ),
    };
    let span = declaration.map_or_else(Span::default, |stmt| stmt.span);
    let args: Vec<Type> = type_args.iter().map(|(_, ty)| ty.clone()).collect();
    let depth = limits::deepest_argument_depth(&args);
    if depth <= limits::MAX_INSTANCE_TYPE_DEPTH {
        let refusal = mir::symbol::ClaimRefusal::Unnameable(call.symbol().clone());
        return Err(CompilerError::Lowering(refusal.refusal(span)));
    }
    let growth = limits::Growth {
        chain: Vec::new(),
        method: None,
        through_trait: false,
    };
    let limit = limits::ExceededLimit::TypeDepth(depth);
    Err(CompilerError::Lowering(limits::polymorphic_recursion(
        name, &args, &limit, &growth, span,
    )))
}

impl Pipeline {
    /// Lower every generic function and method instantiation the bodies from
    /// `first_new` on call, and every one those instantiations call in turn.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_generic_functions_reached_from(
        result: &PipelineResult,
        is_release: bool,
        first_new: usize,
        reach: &ReachTables,
        bodies: &mut Vec<LoweredBody>,
        symbols: &mut SymbolTable,
        compilation_ids: &mir::lowering::SharedCompilationIds,
    ) -> Result<(), CompilerError> {
        let mut pending = std::collections::VecDeque::new();
        Self::queue_generic_instantiations(&bodies[first_new..], symbols, &mut pending);
        while let Some(call) = pending.pop_front() {
            if call.symbol().has_an_unnameable_argument() {
                continue;
            }
            let first_new = bodies.len();
            let lowering = LoweringSink {
                is_release,
                bodies: &mut *bodies,
                symbols: &mut *symbols,
                compilation_ids,
            };
            match call {
                GenericInstantiation::Function(call) => {
                    Self::lower_generic_function_instantiation(result, reach, call, lowering)?;
                }
                GenericInstantiation::Method(call) => {
                    Self::lower_generic_method_instantiation(result, reach, call, lowering)?;
                }
            }
            Self::queue_generic_instantiations(&bodies[first_new..], symbols, &mut pending);
        }
        Ok(())
    }

    /// Lower the generic function `call` reaches at its type arguments, unless
    /// its symbol is already compiled.
    fn lower_generic_function_instantiation(
        result: &PipelineResult,
        reach: &ReachTables,
        call: GenericFunctionCall,
        sink: LoweringSink<'_>,
    ) -> Result<(), CompilerError> {
        let Some(ast_stmt) = reach
            .generic_functions
            .get(&call.function)
            .and_then(|site| site.statement(result))
        else {
            return Ok(());
        };
        if !sink
            .symbols
            .claim_at(&call.symbol, ast_stmt.span)
            .map_err(CompilerError::Lowering)?
        {
            return Ok(());
        }
        let subs = call.type_args.into_iter().collect();
        let body = mir::lowering::lower_generic_instantiation_with_compilation_ids(
            ast_stmt,
            &result.type_checker,
            sink.is_release,
            true,
            &subs,
            sink.compilation_ids.clone(),
        )
        .map_err(|e| monomorphized_lowering_failure(&call.symbol, e))?;
        Self::push_lowered_body(sink.bodies, sink.symbols, call.symbol, body)
    }

    /// Lower the copy of the generic method `call` reaches, unless its symbol
    /// is already compiled. A call whose body no declaration gives is an
    /// internal error: lowering names a copy only for a method it resolved.
    fn lower_generic_method_instantiation(
        result: &PipelineResult,
        reach: &ReachTables,
        call: GenericMethodCall,
        sink: LoweringSink<'_>,
    ) -> Result<(), CompilerError> {
        let Some(method_stmt) = generic_method_statement(result, reach, &call) else {
            return Err(CompilerError::Lowering(LoweringError::internal(
                DiagnosticCode::MirInvalidInstantiationArgument,
                format!(
                    "the copy {} of a generic method was called, but no declaration gives \
                     `{}` of `{}` a body",
                    call.symbol.written(),
                    call.method,
                    call.owner
                ),
                Span::default(),
            )));
        };
        if !sink
            .symbols
            .claim_at(&call.symbol, method_stmt.span)
            .map_err(CompilerError::Lowering)?
        {
            return Ok(());
        }
        let body = mir::lowering::lower_generic_method_copy(
            method_stmt,
            &call,
            &result.type_checker,
            sink.is_release,
            sink.compilation_ids.clone(),
        )
        .map_err(|e| monomorphized_lowering_failure(&call.symbol, e))?;
        Self::push_lowered_body(sink.bodies, sink.symbols, call.symbol, body)
    }

    /// Queue every generic function and method instantiation `bodies` call
    /// that has no body yet, in call order.
    ///
    /// Each call carries its own type arguments, recorded when it was lowered
    /// with the calling body's substitution applied, so a call inside an
    /// instantiation names its callee at that instantiation's types.
    fn queue_generic_instantiations(
        bodies: &[LoweredBody],
        symbols: &SymbolTable,
        pending: &mut std::collections::VecDeque<GenericInstantiation>,
    ) {
        for call in bodies.iter().flat_map(generic_instantiations_called) {
            let already_queued = pending
                .iter()
                .any(|queued| queued.symbol() == call.symbol());
            if !symbols.is_claimed(call.symbol()) && !already_queued {
                pending.push_back(call.queued());
            }
        }
    }
}
