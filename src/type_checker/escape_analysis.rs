// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Escape summary data structures for escape-based use-after-move inference
//! on managed types.
//!
//! A function's escape summary answers the question: *which of my managed-type
//! parameters outlive the call?*  A parameter "escapes" if it is returned,
//! stored into a heap field, captured into a returned closure, or passed to
//! another function that escapes it.
//!
//! # Structure
//!
//! [`EscapeSummary`] bundles three axes:
//!
//! - **`direct_escapes`** — parameter indices that unconditionally escape.
//! - **`conditional_escapes`** — parameters that escape *iff* a fn-typed
//!   parameter has a specific callee-parameter in its own escape set (the
//!   higher-order case).
//! - **`return_aliases`** — parameters whose heap is aliased by the return
//!   value.  At a call site, if the return value itself escapes,
//!   these parameters are also treated as escaping.
//!
//! # Key used in [`super::context::Context::escape_summaries`]
//!
//! Functions are keyed by a [`FunctionId`]: a free function by its name
//! (`save`), a method by the type that declares it together with its name
//! (`Cache` and `store`). The two parts of a method key stay apart, so a
//! method `c` of a type `A_b` and a method `b_c` of a type `A` never share a
//! summary.
//!
//! # Runtime entry points
//!
//! A `runtime "core" fn` declaration has no body to analyse; its summary is
//! read off what the declaration states ([`runtime_escape_summaries`]): the
//! elements it stores escape, and nothing else it is handed does.
//!
//! # Generics strategy
//!
//! Escape analysis runs at type-check time, **pre-monomorphization**, and
//! treats every generic parameter as a typed unknown:
//!
//! - **Managed-bounded or unbounded generics** (`T`, `T extends ManagedClass`,
//!   `T implements SomeTrait`) flow through the same managed-type pathway as
//!   concrete heap types — escape summaries are computed once on the generic
//!   form and apply to every monomorphization.
//! - **Resource-bounded generics** (`T extends ResourceClass` where the bound
//!   class itself defines `fn drop`) inherit the strict-consume rule from
//!   the bound; no escape analysis applies.  This bifurcation is implemented
//!   by [`super::utils::ResourceTypes::holds`], which descends into a
//!   generic parameter's constraint when classifying it.
//!
//! No per-monomorphization re-analysis is required.  Monomorphization
//! specialises *types*, not the call graph; escape rules are structural, so a
//! parameter that escapes for the generic form escapes for every
//! monomorphization, and a parameter that does not escape for the generic
//! form does not escape for any monomorphization.
//!
//! If a future feature breaks this invariant — for example, trait-based
//! fn-valued generics or generic higher-order combinators with type-class-
//! style dispatch — this strategy may need revisiting.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::pattern::Pattern;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::ast::Parameter;
use crate::error::syntax::Span;
use crate::runtime_effects::RuntimeEffects;

use super::context::{class_implements_trait, resolve_method_source, MethodSource, TypeDefinition};
use super::function_analysis::{DeclaredFunction, ModuleId};
use super::statements::returns::collect_tail_statements;
use super::utils::{is_auto_copy, Instantiations, ResourceTypes};
use super::TypeChecker;

/// Zero-based index of a parameter in a function's parameter list.
pub type ParamIndex = usize;

/// The locals a body has bound so far, each with the parameters it may name.
type Locals = HashMap<String, BTreeSet<ParamIndex>>;

/// The summary key of the free function `callee`, named `name`: the function
/// the type checker resolved it to where it is written, or, for a runtime
/// entry point, which the type checker identifies by name alone, its name.
pub(crate) fn free_callee(
    callee: &Expression,
    name: &str,
    callees: &HashMap<usize, DeclaredFunction>,
) -> FunctionId {
    callees.get(&callee.id).map_or_else(
        || FunctionId::free(name),
        |declared| FunctionId::Declared(declared.clone()),
    )
}

/// The function an escape summary describes: the key of
/// [`super::context::Context::escape_summaries`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FunctionId {
    /// A free function — or a runtime function, whose summary its declaration
    /// states — by its declared name.
    Free(String),
    /// A function a module declares, by its declaration: two modules may each
    /// declare a function of one name, and they are two functions.
    Declared(DeclaredFunction),
    /// A method, by the type that declares it and its own name.
    Method { owner: String, method: String },
}

impl FunctionId {
    /// The free function `name`.
    pub fn free(name: &str) -> Self {
        Self::Free(name.to_string())
    }

    /// The method `method` declared by `owner`.
    pub fn method(owner: &str, method: &str) -> Self {
        Self::Method {
            owner: owner.to_string(),
            method: method.to_string(),
        }
    }
}

/// How a diagnostic names the function: a free function by its name, a method
/// as the source spells a call to it, `{owner}.{method}`.
impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FunctionId::Free(name) => f.write_str(name),
            FunctionId::Declared(declared) => f.write_str(&declared.name),
            FunctionId::Method { owner, method } => write!(f, "{owner}.{method}"),
        }
    }
}

/// Describes a single conditional escape: parameter `param` escapes iff the
/// fn-typed parameter at `via_fn_param` has `callee_param` in its own escape
/// set at the call site where it is resolved.
///
/// Read as: *"my parameter `param` escapes through whichever function is
/// passed as argument `via_fn_param`, specifically through that function's
/// parameter slot `callee_param`."*
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalEscape {
    /// Which managed-type parameter of this function flows through a fn-typed arg.
    pub param: ParamIndex,
    /// Which fn-typed parameter carries the escape (index into the same param list).
    pub via_fn_param: ParamIndex,
    /// Which parameter slot of the fn-typed callee `param` is bound to.
    pub callee_param: ParamIndex,
}

/// What a function does with a parameter it keeps, at the first place its
/// body keeps it: one step of the "consumed because" chain a use-after-move
/// report shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeNextHop {
    /// The parameter is passed to `callee` at `param_slot`, which keeps that
    /// slot. Following the callee's summary continues the chain.
    Call {
        callee: FunctionId,
        param_slot: usize,
    },
    /// The parameter is the value the function returns.
    Return,
    /// The parameter is part of an aggregate, a constructed value or a variant
    /// the function returns.
    ReturnAggregate,
    /// The parameter is stored into the field `field`.
    FieldStore { field: String },
    /// The parameter is stored into an element.
    ElementStore,
    /// The parameter is part of a value the function builds and holds.
    Built,
    /// The parameter is captured by a closure, which takes what it captures
    /// when it is created; `returned` when the function returns the closure.
    ClosureCapture { returned: bool },
    /// The parameter is passed to the function value `fn_name`, which this
    /// body cannot see into.
    DynamicFn { fn_name: String },
}

/// Where a function keeps one of its parameters: the step it takes, and the
/// place in the source that takes it, when that place is in the file the
/// program being checked is written in (a report can point only into its own
/// file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepSite {
    pub hop: EscapeNextHop,
    pub span: Option<Span>,
}

/// Escape summary for a single function.
///
/// Computed bottom-up over the call graph by the escape analysis pass
/// (`src/type_checker/escape_analysis.rs`).  A runtime entry point's summary
/// is read off its declaration ([`runtime_escape_summaries`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EscapeSummary {
    /// Parameters that unconditionally escape (returned, stored, captured,
    /// or passed to a definitely-escaping callee).
    pub direct_escapes: BTreeSet<ParamIndex>,
    /// Parameters whose escape depends on a fn-typed argument.
    pub conditional_escapes: Vec<ConditionalEscape>,
    /// Parameters aliased by the return value — if the caller lets the return
    /// value escape, these params escape too.
    pub return_aliases: BTreeSet<ParamIndex>,
    /// For each param in `direct_escapes`, the first place the body keeps
    /// it, recorded by the same walk that finds the keep.
    pub escape_next_hops: HashMap<ParamIndex, KeepSite>,
}

impl EscapeSummary {
    /// Returns `true` if the summary is conservative-empty (nothing escapes).
    pub fn is_empty(&self) -> bool {
        self.direct_escapes.is_empty()
            && self.conditional_escapes.is_empty()
            && self.return_aliases.is_empty()
    }

    /// Returns `true` if parameter `p` unconditionally escapes.
    pub fn directly_escapes(&self, p: ParamIndex) -> bool {
        self.direct_escapes.contains(&p)
    }
}

/// Per-function contribution computed by [`analyze_return_value`] when walking a
/// `return` expression: which parameters escape via the return value, split into
/// direct-consume and return-alias axes.
///
/// Parameter `p` "escapes via return" iff a value `v` is part of
/// the return expression's value and `v` either *is* `p` or aliases `p`'s heap.
/// The two axes capture the same value-flow phenomenon from two angles:
///
/// - `direct_escapes` — `p` is consumed at this function's call site (the
///   caller cannot use `p` again).  This contributes to
///   the overall summary's `direct_escapes`.
/// - `return_aliases` — the return value's heap aliases `p`'s heap; a caller
///   that lets this function's return escape must also treat `p` as escaping.
///   This populates the summary's `return_aliases`.
///
/// The two sets are not redundant: a parameter consumed by a callee inside the
/// return expression (`return f(p)` where `f` consumes its param 0) is in
/// `direct_escapes` but not in `return_aliases` — `f`'s sink retains `p`, but
/// the value flowing into our return is `f`'s independent return value, which
/// does not alias `p`'s heap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReturnFlow {
    /// Parameters that the return expression consumes — directly returned,
    /// indirectly aliased through a managed projection, or passed to a callee
    /// whose summary marks the parameter as escaping.
    pub direct_escapes: BTreeSet<ParamIndex>,
    /// Parameters whose heap is aliased by the return value (a caller storing
    /// the return value also retains these parameters).
    pub return_aliases: BTreeSet<ParamIndex>,
    /// For each parameter in `direct_escapes`, the first place it is kept.
    pub kept_at: BTreeMap<ParamIndex, KeepSite>,
}

impl ReturnFlow {
    /// Records that `param` is kept, at `site` unless an earlier place
    /// already keeps it.
    fn keep(&mut self, param: ParamIndex, site: impl FnOnce() -> KeepSite) {
        self.direct_escapes.insert(param);
        self.kept_at.entry(param).or_insert_with(site);
    }

    /// Records every parameter `held` keeps as kept here too, without the
    /// return aliases `held` saw.
    fn keep_all(&mut self, held: ReturnFlow) {
        for (param, site) in held.kept_at {
            self.keep(param, || site);
        }
    }
}

/// Computes a `ReturnFlow` for a function's return expression.
///
/// This implements the value-flow rule used when
/// determining whether a parameter belongs in the function's
/// [`EscapeSummary::direct_escapes`] (and, as a separate axis, its
/// [`EscapeSummary::return_aliases`]).
///
/// # Arguments
///
/// - `return_expr` — the expression being returned (the operand of the
///   `return` statement, or a function body's tail expression).
/// - `params` — the function's parameter list, used to map identifiers in the
///   return expression back to parameter indices.
/// - `types` — the type-checker's per-expression type map; used to classify
///   intermediate expressions as managed (heap, alias-creating) vs auto-copy
///   (primitive, value-copy at every step).
/// - `type_definitions` — needed by [`is_auto_copy`] to classify struct/enum
///   types; passed through to the predicate.
/// - `escape_summaries` — known callee summaries (FFI sidecar entries plus any
///   summaries already computed for callees in the current SCC).
///   Looked up by free-function name and `ClassName_method` for direct method
///   calls.
///
/// # Rule cases
///
/// 1. `return p` → both sets contain `p`.
/// 2. Aggregate construction (`return [p]`, `Pair(p, q)`, `{k: p}`,
///    `Some(p)`) → recurse into each element with the same alias context;
///    every flowed-into managed param is added to both sets.
/// 3. `return p[i]` — index expression's *result type* decides:
///    - Managed element type → `return_aliases` contains `p`, and
///      `direct_escapes` does **not**: what reaches the return is the element,
///      so the caller keeps `p` and goes on using it, while a caller that lets
///      the return escape must still treat `p` as escaping.
///    - Auto-copy element type → neither set contains `p` (indexing copies).
/// 4. `return p.field` — same split based on the member access's result type.
///    This is what lets a class compose two of its own reads: a getter that
///    returns a field does not consume the object it read it from.
/// 5. `return f(p)` where `f`'s param 0 ∈ `direct_escapes` → `p` ∈
///    `direct_escapes` of the caller (consumed via `f`'s sink chain), but only
///    in `return_aliases` if rule 7 also applies.
/// 6. `return f(p)` where `f`'s param 0 escapes neither directly nor via
///    return alias → `p` ∈ neither set.
/// 7. `return f(p)` where `f`'s `return_aliases` ∋ 0 → `p` ∈ both sets (the
///    call's return value aliases `p`'s heap, so our return aliases `p` too).
///
/// # Out of scope
///
/// This analyzer handles the core value-flow rule for return expressions.
/// Field-store side effects, closure-capture into a returned closure, and
/// transitive call passing independent of the return position are handled
/// elsewhere in the escape analysis pass.
pub fn analyze_return_value(
    return_expr: &Expression,
    params: &[Parameter],
    types: &HashMap<usize, Type>,
    type_definitions: &HashMap<String, TypeDefinition>,
    escape_summaries: &HashMap<FunctionId, EscapeSummary>,
) -> ReturnFlow {
    let resources = ResourceTypes::of(type_definitions, &Instantiations::new());
    let analyzer = ReturnFlowAnalyzer {
        params,
        locals: &HashMap::new(),
        callees: &HashMap::new(),
        types,
        type_definitions,
        resources: &resources,
        escape_summaries,
        keeping: Keeping::Returned,
        at: None,
        wrapped: Cell::new(false),
        captured: Cell::new(false),
    };
    let mut flow = ReturnFlow::default();
    analyzer.classify(return_expr, true, &mut flow);
    flow
}

/// What keeps a parameter the value-flow rule finds in a value: the function
/// returning the value, or the place the value is stored into.
#[derive(Clone)]
enum Keeping {
    Returned,
    Stored(EscapeNextHop),
}

struct ReturnFlowAnalyzer<'a> {
    params: &'a [Parameter],
    /// The locals in scope, each with the parameters it may name.
    locals: &'a Locals,
    /// The function each free-function callee resolved to.
    callees: &'a HashMap<usize, DeclaredFunction>,
    types: &'a HashMap<usize, Type>,
    type_definitions: &'a HashMap<String, TypeDefinition>,
    /// The types that are resources, decided over `type_definitions`.
    resources: &'a ResourceTypes,
    escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    /// What keeps a parameter found in the value classified.
    keeping: Keeping,
    /// The place in the source that keeps it, when a report can point there.
    at: Option<Span>,
    /// Whether the value being classified is part of an aggregate, a
    /// constructed value or a variant rather than the whole value.
    wrapped: Cell<bool>,
    /// Whether the value being classified is captured by a closure.
    captured: Cell<bool>,
}

impl<'a> ReturnFlowAnalyzer<'a> {
    /// The step that keeps a parameter found where the classification is.
    fn hop(&self) -> EscapeNextHop {
        if self.captured.get() {
            let returned = matches!(self.keeping, Keeping::Returned);
            return EscapeNextHop::ClosureCapture { returned };
        }
        match &self.keeping {
            Keeping::Returned if self.wrapped.get() => EscapeNextHop::ReturnAggregate,
            Keeping::Returned => EscapeNextHop::Return,
            Keeping::Stored(hop) => hop.clone(),
        }
    }

    /// `hop`, taken at the place this classification is for.
    fn site(&self, hop: EscapeNextHop) -> KeepSite {
        KeepSite { hop, span: self.at }
    }

    /// Runs `inner` with `flag` set, restoring it after.
    fn flagged(flag: &Cell<bool>, inner: impl FnOnce()) {
        let was = flag.replace(true);
        inner();
        flag.set(was);
    }
    /// Walks `expr` recording escape flow into `flow`.
    ///
    /// `aliases_return` is the alias-context: `true` iff this expression's
    /// value is part of the return value's heap-alias structure (i.e.,
    /// returning this expression directly would mean returning the param it
    /// references).  It is propagated AND'd with managed-ness at each
    /// projection step (`Index`/`Member`), and AND'd with the callee's
    /// `return_aliases` membership at call boundaries.
    fn classify(&self, expr: &Expression, aliases_return: bool, flow: &mut ReturnFlow) {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => {
                self.classify_identifier(name, aliases_return, expr, flow);
            }
            ExpressionKind::List(elems)
            | ExpressionKind::Tuple(elems)
            | ExpressionKind::Set(elems)
            | ExpressionKind::Array(elems, _)
            | ExpressionKind::EnumValue(_, elems) => {
                Self::flagged(&self.wrapped, || {
                    self.classify_elements(elems, aliases_return, flow)
                });
            }
            ExpressionKind::Map(pairs) => {
                Self::flagged(&self.wrapped, || {
                    self.classify_map(pairs, aliases_return, flow)
                });
            }
            ExpressionKind::Index(obj, idx_expr) => {
                self.classify_index(obj, idx_expr, aliases_return, expr, flow);
            }
            ExpressionKind::Member(obj, _) => {
                self.classify_member(obj, aliases_return, expr, flow);
            }
            ExpressionKind::Call(callee, args) => {
                self.classify_call(callee, args, aliases_return, flow);
            }
            ExpressionKind::NamedArgument(_, val) => {
                self.classify(val, aliases_return, flow);
            }
            ExpressionKind::Conditional(then_expr, cond_expr, else_expr, _) => {
                self.classify_conditional(then_expr, cond_expr, else_expr, aliases_return, flow);
            }
            ExpressionKind::Block(_, final_expr) => {
                self.classify(final_expr, aliases_return, flow);
            }
            ExpressionKind::Match(scrutinee, branches) => {
                self.classify_match(scrutinee, branches, aliases_return, flow);
            }
            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.classify_binary_logical(l, r, flow);
            }
            ExpressionKind::Unary(_, e) | ExpressionKind::Guard(_, e) => {
                self.classify(e, false, flow);
            }
            ExpressionKind::Range(start, end, _) => {
                Self::flagged(&self.wrapped, || {
                    self.classify_range(start, end, aliases_return, flow)
                });
            }
            ExpressionKind::FormattedString(parts, _) => {
                self.classify_formatted_string(parts, flow);
            }
            ExpressionKind::Assignment(_, _, rhs) => {
                self.classify(rhs, aliases_return, flow);
            }
            ExpressionKind::Lambda(lambda_data) => {
                if aliases_return {
                    Self::flagged(&self.captured, || {
                        self.scan_lambda_captures(&lambda_data.body, flow)
                    });
                }
            }
            ExpressionKind::Cast(value_expr, _target_type_expr) => {
                self.classify(value_expr, aliases_return, flow);
            }
            // TODO: `super` names the receiver, yet handing it to a keeper
            // (`super.keep()`) does not yet make `self` escape, so a method
            // forwarding its receiver through `super` is not seen keeping it
            // and lends it instead of owning it.
            ExpressionKind::Literal(_)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Super => {}
        }
    }

    fn classify_identifier(
        &self,
        name: &str,
        aliases_return: bool,
        expr: &Expression,
        flow: &mut ReturnFlow,
    ) {
        if aliases_return && self.is_managed_expr(expr) {
            for idx in self.param_indices(name) {
                flow.keep(idx, || self.site(self.hop()));
                flow.return_aliases.insert(idx);
            }
        }
    }

    fn classify_elements(&self, elems: &[Expression], aliases_return: bool, flow: &mut ReturnFlow) {
        for e in elems {
            self.classify(e, aliases_return, flow);
        }
    }

    fn classify_map(
        &self,
        pairs: &[(Expression, Expression)],
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        for (k, v) in pairs {
            self.classify(k, aliases_return, flow);
            self.classify(v, aliases_return, flow);
        }
    }

    /// Record a parameter that reaches the return only *through a projection* —
    /// one of its fields or elements — rather than as the whole value.
    ///
    /// Such a parameter is aliased by the return without being consumed by it:
    /// the value handed back is part of the parameter's heap, so a caller that
    /// lets it escape must treat the parameter as escaping too, but the
    /// parameter itself is untouched and stays usable. Reference counting gives
    /// the projection its own reference, so it outliving the parameter is sound.
    ///
    /// Answers whether it recorded anything, so the caller knows not to walk the
    /// base a second time as a whole value — which is what would consume it.
    fn record_projected_param(&self, obj: &Expression, flow: &mut ReturnFlow) -> bool {
        let ExpressionKind::Identifier(name, _) = &obj.node else {
            return false;
        };
        let mut named = self.param_indices(name).peekable();
        if named.peek().is_none() || !self.is_managed_expr(obj) {
            return false;
        }
        flow.return_aliases.extend(named);
        true
    }

    fn classify_index(
        &self,
        obj: &Expression,
        idx_expr: &Expression,
        aliases_return: bool,
        expr: &Expression,
        flow: &mut ReturnFlow,
    ) {
        let alias_through = aliases_return && self.is_managed_expr(expr);
        self.classify(idx_expr, false, flow);
        if alias_through && self.record_projected_param(obj, flow) {
            return;
        }
        self.classify(obj, alias_through, flow);
    }

    fn classify_member(
        &self,
        obj: &Expression,
        aliases_return: bool,
        expr: &Expression,
        flow: &mut ReturnFlow,
    ) {
        let alias_through = aliases_return && self.is_managed_expr(expr);
        if alias_through && self.record_projected_param(obj, flow) {
            return;
        }
        self.classify(obj, alias_through, flow);
    }

    fn classify_conditional(
        &self,
        then_expr: &Expression,
        cond_expr: &Expression,
        else_expr: &Option<Box<Expression>>,
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        self.classify(cond_expr, false, flow);
        self.classify(then_expr, aliases_return, flow);
        if let Some(e) = else_expr {
            self.classify(e, aliases_return, flow);
        }
    }

    fn classify_match(
        &self,
        scrutinee: &Expression,
        branches: &[crate::ast::MatchBranch],
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        self.classify(scrutinee, false, flow);
        for branch in branches {
            if let Some(guard) = &branch.guard {
                self.classify(guard, false, flow);
            }
            self.classify_stmt_for_value(&branch.body, aliases_return, flow);
        }
    }

    fn classify_binary_logical(&self, l: &Expression, r: &Expression, flow: &mut ReturnFlow) {
        self.classify(l, false, flow);
        self.classify(r, false, flow);
    }

    /// A range's bounds are the fields of the `Range` struct it builds, so a
    /// range follows the rule a struct constructor's arguments do: each bound
    /// is part of the value built, and `xs..xs` agrees with
    /// `Range(start: xs, end: xs, ...)`. A call only ever consumes a
    /// resource, so a collection bound stays the caller's.
    fn classify_range(
        &self,
        start: &Expression,
        end: &Option<Box<Expression>>,
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        self.classify(start, aliases_return, flow);
        if let Some(e) = end {
            self.classify(e, aliases_return, flow);
        }
    }

    fn classify_formatted_string(&self, parts: &[Expression], flow: &mut ReturnFlow) {
        for p in parts {
            self.classify(p, false, flow);
        }
    }

    /// Pulls a value-producing expression out of a match-branch body and
    /// classifies it.  Branch bodies are statements; the value-producing form
    /// is either an `Expression` or a `Block` with a trailing expression.
    fn classify_stmt_for_value(
        &self,
        stmt: &crate::ast::Statement,
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        match &stmt.node {
            StatementKind::Expression(e) => self.classify(e, aliases_return, flow),
            StatementKind::Block(stmts) => {
                if let Some(last) = stmts.last() {
                    self.classify_stmt_for_value(last, aliases_return, flow);
                }
            }
            StatementKind::Return(Some(e)) => self.classify(e, aliases_return, flow),
            _ => {}
        }
    }

    /// Scans a lambda body statement for references to managed params.
    ///
    /// Called when a lambda is returned from a function (closure-capture escape).
    /// Any managed param referenced inside the lambda body is captured by the
    /// closure and therefore escapes the enclosing function.
    fn scan_lambda_captures(&self, stmt: &crate::ast::Statement, flow: &mut ReturnFlow) {
        match &stmt.node {
            StatementKind::Expression(e) => self.scan_lambda_expr(e, flow),
            StatementKind::Block(stmts) => {
                for s in stmts {
                    self.scan_lambda_captures(s, flow);
                }
            }
            StatementKind::Return(Some(e)) => self.scan_lambda_expr(e, flow),
            StatementKind::Variable(decls, _) => {
                for d in decls {
                    if let Some(init) = &d.initializer {
                        self.scan_lambda_expr(init, flow);
                    }
                }
            }
            _ => {}
        }
    }

    /// Walks an expression inside a lambda body looking for managed param references.
    fn scan_lambda_expr(&self, expr: &Expression, flow: &mut ReturnFlow) {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => self.classify_identifier(name, true, expr, flow),
            ExpressionKind::Call(callee, args) => {
                self.scan_lambda_expr(callee, flow);
                for a in args {
                    self.scan_lambda_expr(a, flow);
                }
            }
            ExpressionKind::Member(obj, _) => self.scan_lambda_expr(obj, flow),
            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.scan_lambda_expr(l, flow);
                self.scan_lambda_expr(r, flow);
            }
            ExpressionKind::Unary(_, e) | ExpressionKind::Guard(_, e) => {
                self.scan_lambda_expr(e, flow);
            }
            ExpressionKind::Index(obj, idx) => {
                self.scan_lambda_expr(obj, flow);
                self.scan_lambda_expr(idx, flow);
            }
            ExpressionKind::FormattedString(parts, _) => {
                for p in parts {
                    self.scan_lambda_expr(p, flow);
                }
            }
            ExpressionKind::NamedArgument(_, val) => self.scan_lambda_expr(val, flow),
            _ => {}
        }
    }

    fn classify_call(
        &self,
        callee: &Expression,
        args: &[Expression],
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        let summary_owned = self.resolve_callee_summary(callee);
        let summary = summary_owned.as_ref();

        // Method call: receiver is summary param 0, args shift by 1.
        // Free function call: arg i is summary param i.
        let (receiver, arg_offset) = match &callee.node {
            ExpressionKind::Member(obj, _) => (Some(obj.as_ref()), 1),
            _ => (None, 0),
        };

        if let (Some(s), Some(callee_id)) = (summary, self.callee_id(callee)) {
            let passed = |slot| EscapeNextHop::Call {
                callee: callee_id.clone(),
                param_slot: slot,
            };
            if let Some(obj) = receiver {
                self.apply_summary_for_arg(obj, 0, s, passed(0), aliases_return, flow);
            }
            for (i, arg) in args.iter().enumerate() {
                let summary_idx = i + arg_offset;
                let hop = passed(summary_idx);
                self.apply_summary_for_arg(arg, summary_idx, s, hop, aliases_return, flow);
            }
        } else if let Some(fn_name) = self.parameter_called(callee) {
            // A call through a function-typed parameter reaches a function
            // this body cannot see: it may keep any argument, and its result
            // may be any of them.
            for arg in args {
                let hop = EscapeNextHop::DynamicFn {
                    fn_name: fn_name.to_string(),
                };
                self.consume_leaf_identifier(arg, hop, flow);
                self.classify(arg, aliases_return, flow);
            }
        } else if call_builds_a_value(callee, self.type_definitions) {
            // The value a constructor builds holds every argument it is
            // handed, as a returned tuple holds its elements.
            Self::flagged(&self.wrapped, || {
                for arg in args {
                    self.classify(arg, aliases_return, flow);
                }
            });
        } else {
            // No summary available — this analyzer alone cannot decide whether
            // the call propagates flow.  For value-flow purposes, we
            // recurse without propagating alias context: nested patterns
            // inside arg expressions still get classified, but we make no
            // claim about the call's own return value.
            if let Some(obj) = receiver {
                self.classify(obj, false, flow);
            }
            for arg in args {
                self.classify(arg, false, flow);
            }
        }
    }

    /// Applies a callee's escape summary to one argument expression.
    fn apply_summary_for_arg(
        &self,
        arg: &Expression,
        summary_idx: ParamIndex,
        summary: &EscapeSummary,
        hop: EscapeNextHop,
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        // Rule 5: callee consumes this slot — mark the leaf identifier (if
        // a managed param) as direct_escape regardless of return-alias state.
        if summary.direct_escapes.contains(&summary_idx) {
            self.consume_leaf_identifier(arg, hop, flow);
        }

        // Rule 7: the call's return value aliases this slot's heap.  If our
        // return position aliases the call's return, then the slot's argument
        // also flows into our return alias chain.
        let arg_aliases_return = aliases_return && summary.return_aliases.contains(&summary_idx);

        // Rule 6 falls out: if neither direct_escapes nor return_aliases
        // matches this slot, the recursive call below uses
        // arg_aliases_return = false, contributing nothing.
        self.classify(arg, arg_aliases_return, flow);
    }

    /// If `arg` (or its `NamedArgument` wrapper) is a leaf identifier referring
    /// to a managed-typed parameter, records that parameter as kept by `hop`.
    fn consume_leaf_identifier(&self, arg: &Expression, hop: EscapeNextHop, flow: &mut ReturnFlow) {
        let inner = argument_value(arg);
        if let ExpressionKind::Identifier(name, _) = &inner.node {
            if self.is_managed_expr(inner) {
                for idx in self.param_indices(name) {
                    flow.keep(idx, || self.site(hop.clone()));
                }
            }
        }
    }

    /// The function a hop through a call of `callee` names: a free function
    /// as it resolved, a method as [`hop_method`] names it.
    fn callee_id(&self, callee: &Expression) -> Option<FunctionId> {
        if let ExpressionKind::Identifier(name, _) = &callee.node {
            return Some(self.free_callee(callee, name));
        }
        let ExpressionKind::Member(receiver, method) = &callee.node else {
            return None;
        };
        let (type_name, method_name) = method_call_target(receiver, method, self.types)?;
        Some(hop_method(
            type_name,
            method_name,
            self.escape_summaries,
            self.type_definitions,
        ))
    }

    /// The zero-based indices of the parameters `name` may denote: a local in
    /// scope denotes the parameters it may have been bound from, none for a
    /// local shadowing a parameter; any other name denotes the parameter it
    /// spells, if any.  `self` is index 0 for methods (mirroring the mangling
    /// convention used elsewhere); free functions use the explicit parameter
    /// list.
    fn param_indices<'s>(&'s self, name: &str) -> impl Iterator<Item = ParamIndex> + 's {
        let local = self.locals.get(name);
        let param = local
            .is_none()
            .then(|| self.params.iter().position(|p| p.name == name))
            .flatten();
        local.into_iter().flatten().copied().chain(param)
    }

    /// The summary key of the free function `callee`, named `name`, resolved
    /// to ([`free_callee`]).
    fn free_callee(&self, callee: &Expression, name: &str) -> FunctionId {
        free_callee(callee, name, self.callees)
    }

    /// The name of the function value `callee` calls when it names one of
    /// this function's parameters, i.e. the call goes through a function
    /// value the caller supplied.
    fn parameter_called<'e>(&self, callee: &'e Expression) -> Option<&'e str> {
        let ExpressionKind::Identifier(name, _) = &callee.node else {
            return None;
        };
        self.param_indices(name).next().map(|_| name.as_str())
    }

    /// Returns `true` if a caller can tell whether `expr` is kept: its type is
    /// managed (heap-allocated) or a resource. Other auto-copy values
    /// (primitives, small all-scalar structs) are copied bytes that cannot alias
    /// another heap; a resource is tracked whatever its layout, because its hook
    /// runs once and the caller must know whether the callee now owns it.
    /// Whether the value of `expr` is held by reference: a counted value, a
    /// resource, or a closure, whose environment is counted though the
    /// language copies the closure itself.
    fn is_managed_expr(&self, expr: &Expression) -> bool {
        match self.types.get(&expr.id) {
            Some(ty) => {
                matches!(ty.kind, TypeKind::Function(_))
                    || !is_auto_copy(&ty.kind, self.type_definitions)
                    || self.resources.holds(&ty.kind)
            }
            None => false,
        }
    }

    /// Looks up the escape summary for `callee`: a free function's, or the
    /// one a method call reaches through the receiver's type
    /// ([`method_summary`]).
    fn resolve_callee_summary(&self, callee: &Expression) -> Option<EscapeSummary> {
        match &callee.node {
            ExpressionKind::Identifier(name, _) => {
                let key = self.free_callee(callee, name);
                self.escape_summaries.get(&key).cloned()
            }
            ExpressionKind::Member(obj, method_expr) => self.method_call_summary(obj, method_expr),
            _ => None,
        }
    }

    /// The summary a call of `method` on `receiver` reaches ([`method_summary`]).
    fn method_call_summary(
        &self,
        receiver: &Expression,
        method: &Expression,
    ) -> Option<EscapeSummary> {
        let (type_name, method_name) = method_call_target(receiver, method, self.types)?;
        method_summary(
            type_name,
            method_name,
            self.escape_summaries,
            self.type_definitions,
        )
    }
}

/// The escape summary of every runtime entry point `effects` describes, as
/// its record states it: a stored element escapes into the collection that
/// keeps it, and nothing else an entry point is handed outlives the call —
/// it is read, written in place, or a closure the runtime calls and drops.
pub fn runtime_escape_summaries(effects: &RuntimeEffects) -> HashMap<FunctionId, EscapeSummary> {
    (effects.iter())
        .map(|(name, effect)| {
            let summary = EscapeSummary {
                direct_escapes: effect.stored_positions().into_iter().collect(),
                ..EscapeSummary::default()
            };
            (FunctionId::free(name), summary)
        })
        .collect()
}

/// Whether a call to `callee` builds a value out of its arguments: a struct or
/// class constructor, an enum variant, or a builtin variant constructor. The
/// value built holds every argument, so building one keeps each argument,
/// wherever the value goes. Escape analysis and the use-after-move check both
/// ask this.
pub(crate) fn call_builds_a_value(
    callee: &Expression,
    type_defs: &HashMap<String, TypeDefinition>,
) -> bool {
    if let ExpressionKind::Identifier(name, _) = &callee.node {
        return TypeChecker::is_builtin_variant_constructor(name)
            || matches!(
                type_defs.get(name),
                Some(TypeDefinition::Struct(_) | TypeDefinition::Class(_))
            );
    }
    if let ExpressionKind::Member(owner, _) = &callee.node {
        return matches!(&owner.node, ExpressionKind::Identifier(name, _)
            if matches!(type_defs.get(name), Some(TypeDefinition::Enum(_))));
    }
    false
}

/// The receiver's declared type and the method's name of a method call, when
/// the receiver is of a named type.
fn method_call_target<'e>(
    receiver: &Expression,
    method: &'e Expression,
    types: &'e HashMap<usize, Type>,
) -> Option<(&'e str, &'e str)> {
    let ExpressionKind::Identifier(method_name, _) = &method.node else {
        return None;
    };
    let TypeKind::Custom(type_name, _) = &types.get(&receiver.id)?.kind else {
        return None;
    };
    Some((type_name, method_name))
}

/// The method a hop through a call of `method` on a receiver of type
/// `type_name` names: the one body the call runs, which the chain follows on;
/// the method as the receiver's type declares it when the call may run
/// several, or goes through a trait.
fn hop_method(
    type_name: &str,
    method: &str,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    type_defs: &HashMap<String, TypeDefinition>,
) -> FunctionId {
    let has_body = |id: &FunctionId| summaries.contains_key(id);
    let through_trait = matches!(type_defs.get(type_name), Some(TypeDefinition::Trait(_)));
    match method_targets(type_name, method, &has_body, type_defs).as_slice() {
        [only] if !through_trait => only.clone(),
        _ => FunctionId::method(type_name, method),
    }
}

/// The escape summary a call of `method` on a receiver of type `type_name`
/// reaches: the summary of the type that defines it, or, for a call through
/// a trait or an abstract class, the union over every concrete class the call
/// may dispatch to. `None` when the call reaches no analysed method, which a
/// call site reads as a callee that keeps every resource it is handed.
pub(crate) fn method_summary(
    type_name: &str,
    method: &str,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<EscapeSummary> {
    let has_summary = |id: &FunctionId| summaries.contains_key(id);
    let targets = method_targets(type_name, method, &has_summary, type_defs);
    if let [only] = targets.as_slice() {
        return summaries.get(only).cloned();
    }
    let mut joined: Option<EscapeSummary> = None;
    for summary in targets.iter().filter_map(|target| summaries.get(target)) {
        let into = joined.get_or_insert_with(EscapeSummary::default);
        into.direct_escapes.extend(&summary.direct_escapes);
        into.return_aliases.extend(&summary.return_aliases);
        let conditional = summary.conditional_escapes.iter().cloned();
        into.conditional_escapes.extend(conditional);
    }
    joined
}

/// The methods a call of `method` on a receiver of type `type_name` may run:
/// a struct's or an enum's own; for a class, the body it runs and the body
/// every concrete class extending it runs, since the receiver may be any of
/// them; for a trait, the body each concrete implementor runs. `has_body` says
/// which methods exist to be run.
fn method_targets(
    type_name: &str,
    method: &str,
    has_body: &dyn Fn(&FunctionId) -> bool,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Vec<FunctionId> {
    match type_defs.get(type_name) {
        Some(TypeDefinition::Trait(_)) | Some(TypeDefinition::Class(_)) => {
            implementor_methods(type_name, method, has_body, type_defs)
        }
        Some(TypeDefinition::Struct(_)) | Some(TypeDefinition::Enum(_)) | None => {
            method_owner(type_name, method, has_body, type_defs)
                .into_iter()
                .collect()
        }
        Some(TypeDefinition::Generic(_)) | Some(TypeDefinition::Alias(_)) => Vec::new(),
    }
}

/// The method each concrete class implementing the trait, or extending the
/// class, `parent` runs for `method`.
///
/// Membership is the shared answer, which walks a class's whole ancestry and
/// every trait each step of it extends: leaving out one implementor while
/// finding another would report as absent an escape the class the call
/// dispatches to has.
fn implementor_methods(
    parent: &str,
    method: &str,
    has_body: &dyn Fn(&FunctionId) -> bool,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Vec<FunctionId> {
    let mut targets: Vec<FunctionId> = (type_defs.iter())
        .filter(|(class_name, td)| {
            matches!(td, TypeDefinition::Class(cd) if !cd.is_abstract)
                && (class_implements_trait(class_name, parent, type_defs)
                    || class_extends(class_name, parent, type_defs))
        })
        .filter_map(|(class_name, _)| method_owner(class_name, method, has_body, type_defs))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// The method a receiver of type `type_name` runs for `method`: for a class,
/// the body [`resolve_method_source`] resolves it to — the nearest class's
/// own, or the trait default it inherits — and a struct's or an enum's own
/// otherwise. `None` when that body is not one `has_body` knows.
pub(crate) fn method_owner(
    type_name: &str,
    method: &str,
    has_body: &dyn Fn(&FunctionId) -> bool,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<FunctionId> {
    let owner = match type_defs.get(type_name) {
        Some(TypeDefinition::Class(_)) => {
            match resolve_method_source(type_defs, type_name, method)? {
                MethodSource::Declared { class, .. } => FunctionId::method(class, method),
                MethodSource::Default(default) => FunctionId::method(default.trait_name, method),
                MethodSource::AbstractOnly { .. } => return None,
            }
        }
        Some(TypeDefinition::Struct(_))
        | Some(TypeDefinition::Enum(_))
        | Some(TypeDefinition::Trait(_))
        | Some(TypeDefinition::Generic(_))
        | Some(TypeDefinition::Alias(_))
        | None => FunctionId::method(type_name, method),
    };
    has_body(&owner).then_some(owner)
}

/// Whether `ancestor` is `class_name` or on its base-class chain.
fn class_extends(
    class_name: &str,
    ancestor: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> bool {
    let mut current = class_name;
    loop {
        if current == ancestor {
            return true;
        }
        match type_defs.get(current) {
            Some(TypeDefinition::Class(cd)) => match &cd.base_class {
                Some(base) => current = base.as_str(),
                None => return false,
            },
            Some(TypeDefinition::Struct(_))
            | Some(TypeDefinition::Enum(_))
            | Some(TypeDefinition::Generic(_))
            | Some(TypeDefinition::Alias(_))
            | Some(TypeDefinition::Trait(_))
            | None => return false,
        }
    }
}

/// Captures a function's parameter list (with `self` at index 0 for methods)
/// and a reference to its body.
struct FunctionDef<'a> {
    params: Vec<Parameter>,
    body: &'a Statement,
    /// The tail expressions the body ends on that it returns
    /// ([`returned_tails`]).
    tails: Vec<&'a Expression>,
    /// Whether the body is written in the file of the program being checked.
    located: bool,
}

/// Computes escape summaries for every user-defined function in `stmts`,
/// merges them with the runtime entry points' `runtime_summaries`, and returns the combined
/// map.  Analysis is bottom-up over the call graph (Tarjan SCC + fixpoint
/// within each SCC).
///
/// Only user functions whose bodies are visible in `program` or `imported`
/// are analysed; a place a body in `program` keeps a parameter carries its
/// location, which a report on the program can point at. A runtime
/// declaration is represented by its summary in `runtime_summaries`.
pub(crate) fn compute_escape_summaries<'a>(
    program: impl IntoIterator<Item = &'a Statement>,
    imported: impl IntoIterator<Item = &'a Statement>,
    facts: &ProgramFacts<'_>,
    runtime_summaries: HashMap<FunctionId, EscapeSummary>,
) -> HashMap<FunctionId, EscapeSummary> {
    let mut summaries = runtime_summaries;

    // Step 1: collect all user function definitions.
    let mut fn_defs: HashMap<FunctionId, FunctionDef<'_>> = HashMap::new();
    collect_function_defs(program, &mut fn_defs, None, facts.modules, true);
    collect_function_defs(imported, &mut fn_defs, None, facts.modules, false);

    if fn_defs.is_empty() {
        return summaries;
    }

    // Step 2: build a call graph over known user functions.
    let call_graph = build_call_graph(&fn_defs, facts);

    // Step 3: Tarjan SCC — returns SCCs with leaf SCCs first (bottom-up order).
    let fn_ids: Vec<FunctionId> = fn_defs.keys().cloned().collect();
    let sccs = tarjan_sccs(&fn_ids, &call_graph);

    // Step 4: process each SCC in order, fixpointing within the SCC.
    for scc in &sccs {
        loop {
            let mut changed = false;
            for fn_id in scc {
                let Some(def) = fn_defs.get(fn_id) else {
                    continue;
                };
                let new_summary = compute_one_summary(def, facts, &summaries);
                // An empty summary is stored too: "keeps nothing" is a fact a
                // caller reads, and only a function with no summary at all is
                // one the caller cannot see into.
                if summaries.get(fn_id) != Some(&new_summary) {
                    summaries.insert(fn_id.clone(), new_summary);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    summaries
}

/// Collects every function body in `stmts`: a free function under the
/// declaration it is ([`FunctionId::Declared`]), and a method — a trait's
/// default included — under the type that declares it.
fn collect_function_defs<'a>(
    stmts: impl IntoIterator<Item = &'a Statement>,
    fn_defs: &mut HashMap<FunctionId, FunctionDef<'a>>,
    class_name: Option<&str>,
    modules: &HashMap<usize, ModuleId>,
    located: bool,
) {
    for stmt in stmts {
        match &stmt.node {
            StatementKind::FunctionDeclaration(decl) => {
                let Some(body) = decl.body.as_deref() else {
                    continue;
                };
                let fn_id = match (class_name, modules.get(&stmt.id)) {
                    (Some(cls), _) => FunctionId::method(cls, &decl.name),
                    (None, Some(module)) => FunctionId::Declared(DeclaredFunction {
                        module: module.clone(),
                        name: decl.name.clone(),
                    }),
                    (None, None) => FunctionId::free(&decl.name),
                };
                let params = match class_name {
                    Some(_) => {
                        let mut p = vec![synthetic_self_param()];
                        p.extend_from_slice(decl.explicit_params());
                        p
                    }
                    None => decl.params.clone(),
                };
                let tails = returned_tails(decl.return_type.as_deref(), body);
                fn_defs.insert(
                    fn_id,
                    FunctionDef {
                        params,
                        body,
                        tails,
                        located,
                    },
                );
            }
            StatementKind::Class(cd) => {
                if let Some(cls) = extract_name_from_expr(&cd.name) {
                    collect_function_defs(&cd.body, fn_defs, Some(&cls), modules, located);
                }
            }
            StatementKind::Struct(name_expr, _, _, methods, _, _)
            | StatementKind::Trait(name_expr, _, _, methods, _) => {
                if let Some(s) = extract_name_from_expr(name_expr) {
                    collect_function_defs(methods, fn_defs, Some(&s), modules, located);
                }
            }
            StatementKind::Block(stmts) => {
                collect_function_defs(stmts, fn_defs, class_name, modules, located);
            }
            _ => {}
        }
    }
}

/// Creates a synthetic `self` parameter used as index-0 in method param lists.
fn synthetic_self_param() -> Parameter {
    use crate::ast::factory::{expr_with_span, make_type};
    Parameter {
        name_span: Default::default(),
        name: "self".to_string(),
        typ: Box::new(expr_with_span(
            ExpressionKind::Type(Box::new(make_type(TypeKind::Void)), false),
            Span::new(0, 0),
        )),
        guard: None,
        default_value: None,
        is_out: false,
        residency: None,
    }
}

/// Extracts a plain class/struct name string from a declaration name expression.
fn extract_name_from_expr(expr: &Expression) -> Option<String> {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => Some(name.clone()),
        ExpressionKind::TypeDeclaration(inner, _, _, _) => {
            if let ExpressionKind::Identifier(name, _) = &inner.node {
                Some(name.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// What the call-graph walk reads: the type of each receiver, the type
/// definitions a method call is resolved through, and the functions whose
/// bodies are analysed.
struct CallGraphScope<'a> {
    types: &'a HashMap<usize, Type>,
    type_defs: &'a HashMap<String, TypeDefinition>,
    callees: &'a HashMap<usize, DeclaredFunction>,
    known: HashSet<&'a FunctionId>,
}

/// Builds a call graph over the known user function IDs.  An edge `f → g`
/// exists when function `f`'s body contains a call that resolves to `g`.
/// Only edges to other user functions (present in `fn_defs`) are recorded;
/// edges to FFI or unknown functions are not needed here (their summaries are
/// already in the `summaries` map and consulted directly during summary
/// computation).
fn build_call_graph<'a>(
    fn_defs: &HashMap<FunctionId, FunctionDef<'a>>,
    facts: &ProgramFacts<'_>,
) -> HashMap<FunctionId, Vec<FunctionId>> {
    let scope = CallGraphScope {
        types: facts.types,
        type_defs: facts.type_defs,
        callees: facts.callees,
        known: fn_defs.keys().collect(),
    };
    let mut graph: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();

    for (fn_id, def) in fn_defs {
        let mut callees: Vec<FunctionId> = Vec::new();
        collect_callees_from_stmt(def.body, &scope, &mut callees);
        // Deduplicate (a callee may appear multiple times).
        callees.sort_unstable();
        callees.dedup();
        graph.insert(fn_id.clone(), callees);
    }

    graph
}

fn collect_callees_from_stmt(
    stmt: &Statement,
    scope: &CallGraphScope<'_>,
    out: &mut Vec<FunctionId>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                collect_callees_from_stmt(s, scope, out);
            }
        }
        StatementKind::Expression(e) => {
            collect_callees_from_expr(e, scope, out);
        }
        StatementKind::Return(Some(e)) => {
            collect_callees_from_expr(e, scope, out);
        }
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    collect_callees_from_expr(init, scope, out);
                }
            }
        }
        StatementKind::If(cond, then, else_, _) => {
            collect_callees_from_expr(cond, scope, out);
            collect_callees_from_stmt(then, scope, out);
            if let Some(e) = else_ {
                collect_callees_from_stmt(e, scope, out);
            }
        }
        StatementKind::While(cond, body, _) => {
            collect_callees_from_expr(cond, scope, out);
            collect_callees_from_stmt(body, scope, out);
        }
        StatementKind::For(_, iter, body) | StatementKind::GpuFrame(_, iter, body) => {
            collect_callees_from_expr(iter, scope, out);
            collect_callees_from_stmt(body, scope, out);
        }
        StatementKind::Forall {
            iterable: iter,
            body,
            ..
        } => {
            collect_callees_from_expr(iter, scope, out);
            collect_callees_from_stmt(body, scope, out);
        }
        StatementKind::GpuFrameBlock(body) => {
            collect_callees_from_stmt(body, scope, out);
        }
        StatementKind::FunctionDeclaration(_)
        | StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Return(None)
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::Enum(..)
        | StatementKind::Struct(..)
        | StatementKind::Class(_)
        | StatementKind::Trait(..)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => {}
    }
}

fn collect_callees_from_expr(
    expr: &Expression,
    scope: &CallGraphScope<'_>,
    out: &mut Vec<FunctionId>,
) {
    match &expr.node {
        ExpressionKind::Call(callee, args) => {
            collect_callees_call(callee, args, scope, out);
        }
        ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
            collect_callees_from_expr(l, scope, out);
            collect_callees_from_expr(r, scope, out);
        }
        ExpressionKind::Unary(_, e) | ExpressionKind::Guard(_, e) => {
            collect_callees_from_expr(e, scope, out);
        }
        ExpressionKind::Index(obj, idx) => {
            collect_callees_from_expr(obj, scope, out);
            collect_callees_from_expr(idx, scope, out);
        }
        ExpressionKind::Member(obj, _) => collect_callees_from_expr(obj, scope, out),
        ExpressionKind::Conditional(then, cond, else_, _) => {
            collect_callees_from_expr(cond, scope, out);
            collect_callees_from_expr(then, scope, out);
            if let Some(e) = else_ {
                collect_callees_from_expr(e, scope, out);
            }
        }
        ExpressionKind::Block(stmts, e) => {
            for s in stmts {
                collect_callees_from_stmt(s, scope, out);
            }
            collect_callees_from_expr(e, scope, out);
        }
        ExpressionKind::Match(sc, branches) => {
            collect_callees_from_expr(sc, scope, out);
            for b in branches {
                if let Some(g) = &b.guard {
                    collect_callees_from_expr(g, scope, out);
                }
                collect_callees_from_stmt(&b.body, scope, out);
            }
        }
        ExpressionKind::List(elems) | ExpressionKind::Set(elems) | ExpressionKind::Tuple(elems) => {
            collect_callees_from_elems(elems, scope, out);
        }
        ExpressionKind::Array(elems, _) => {
            collect_callees_from_elems(elems, scope, out);
        }
        ExpressionKind::Map(pairs) => {
            collect_callees_from_pairs(pairs, scope, out);
        }
        ExpressionKind::EnumValue(_, vals) => {
            collect_callees_from_elems(vals, scope, out);
        }
        ExpressionKind::Assignment(_, _, rhs) => collect_callees_from_expr(rhs, scope, out),
        ExpressionKind::NamedArgument(_, val) => collect_callees_from_expr(val, scope, out),
        ExpressionKind::FormattedString(parts, _) => {
            collect_callees_from_elems(parts, scope, out);
        }
        ExpressionKind::Range(start, end_, _) => {
            collect_callees_from_expr(start, scope, out);
            if let Some(e) = end_ {
                collect_callees_from_expr(e, scope, out);
            }
        }
        ExpressionKind::Cast(value_expr, _target_type_expr) => {
            collect_callees_from_expr(value_expr, scope, out);
        }
        ExpressionKind::Lambda(_)
        | ExpressionKind::Identifier(_, _)
        | ExpressionKind::Literal(_)
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::Super => {}
    }
}

fn collect_callees_from_elems(
    elems: &[Expression],
    scope: &CallGraphScope<'_>,
    out: &mut Vec<FunctionId>,
) {
    for e in elems {
        collect_callees_from_expr(e, scope, out);
    }
}

fn collect_callees_from_pairs(
    pairs: &[(Expression, Expression)],
    scope: &CallGraphScope<'_>,
    out: &mut Vec<FunctionId>,
) {
    for (k, v) in pairs {
        collect_callees_from_expr(k, scope, out);
        collect_callees_from_expr(v, scope, out);
    }
}

fn collect_callees_call(
    callee: &Expression,
    args: &[Expression],
    scope: &CallGraphScope<'_>,
    out: &mut Vec<FunctionId>,
) {
    match &callee.node {
        ExpressionKind::Identifier(name, _) => {
            let id = free_callee(callee, name, scope.callees);
            if scope.known.contains(&id) {
                out.push(id);
            }
        }
        ExpressionKind::Member(obj, method_expr) => {
            if let Some((type_name, method)) = method_call_target(obj, method_expr, scope.types) {
                let has_body = |id: &FunctionId| scope.known.contains(id);
                out.extend(method_targets(
                    type_name,
                    method,
                    &has_body,
                    scope.type_defs,
                ));
            }
            collect_callees_from_expr(obj, scope, out);
        }
        _ => {
            collect_callees_from_expr(callee, scope, out);
        }
    }
    for a in args {
        collect_callees_from_expr(a, scope, out);
    }
}

/// Returns SCCs in topological order with leaf SCCs (no outgoing cross-SCC
/// edges) first.  This is the natural output order of Tarjan's algorithm.
fn tarjan_sccs(
    nodes: &[FunctionId],
    edges: &HashMap<FunctionId, Vec<FunctionId>>,
) -> Vec<Vec<FunctionId>> {
    let mut index = 0usize;
    let mut stack: Vec<FunctionId> = Vec::new();
    let mut on_stack: HashSet<FunctionId> = HashSet::new();
    let mut indices: HashMap<FunctionId, usize> = HashMap::new();
    let mut lowlinks: HashMap<FunctionId, usize> = HashMap::new();
    let mut sccs: Vec<Vec<FunctionId>> = Vec::new();

    for node in nodes {
        if !indices.contains_key(node) {
            tarjan_visit(
                node,
                edges,
                &mut index,
                &mut stack,
                &mut on_stack,
                &mut indices,
                &mut lowlinks,
                &mut sccs,
            );
        }
    }

    sccs
}

#[allow(clippy::too_many_arguments)]
fn tarjan_visit(
    v: &FunctionId,
    edges: &HashMap<FunctionId, Vec<FunctionId>>,
    index: &mut usize,
    stack: &mut Vec<FunctionId>,
    on_stack: &mut HashSet<FunctionId>,
    indices: &mut HashMap<FunctionId, usize>,
    lowlinks: &mut HashMap<FunctionId, usize>,
    sccs: &mut Vec<Vec<FunctionId>>,
) {
    indices.insert(v.clone(), *index);
    lowlinks.insert(v.clone(), *index);
    *index += 1;
    stack.push(v.clone());
    on_stack.insert(v.clone());

    let neighbors: Vec<FunctionId> = edges.get(v).cloned().unwrap_or_default();
    for w in &neighbors {
        if !indices.contains_key(w) {
            tarjan_visit(w, edges, index, stack, on_stack, indices, lowlinks, sccs);
            let w_low = lowlinks.get(w).copied();
            if let (Some(w_low), Some(v_low)) = (w_low, lowlinks.get_mut(v)) {
                *v_low = (*v_low).min(w_low);
            }
        } else if on_stack.contains(w) {
            let w_idx = indices.get(w).copied();
            if let (Some(w_idx), Some(v_low)) = (w_idx, lowlinks.get_mut(v)) {
                *v_low = (*v_low).min(w_idx);
            }
        }
    }

    if lowlinks.get(v) == indices.get(v) {
        let mut scc = Vec::new();
        while let Some(w) = stack.pop() {
            on_stack.remove(&w);
            let done = w == *v;
            scc.push(w);
            if done {
                break;
            }
        }
        sccs.push(scc);
    }
}

/// Computes the escape summary for a single function given the current state of
/// the summaries map (which may contain partial results for the same SCC).
///
/// A parameter escapes when the function keeps it, whichever way that is
/// spelled: it reaches the value the function returns (after `return`, or as
/// the tail expression a body declared to return a value ends on), it is
/// stored into a field or an element, or it is passed to a parameter the next
/// callee keeps.
fn compute_one_summary(
    def: &FunctionDef<'_>,
    facts: &ProgramFacts<'_>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
) -> EscapeSummary {
    let mut walk = EscapeWalk {
        params: &def.params,
        facts,
        summaries,
        tails: def.tails.iter().map(|tail| tail.id).collect(),
        scopes: LocalScopes::default(),
        located: def.located,
    };
    let mut flow = ReturnFlow::default();
    walk.statement(def.body, &mut flow);
    EscapeSummary {
        direct_escapes: flow.direct_escapes,
        conditional_escapes: vec![],
        return_aliases: flow.return_aliases,
        escape_next_hops: flow.kept_at.into_iter().collect(),
    }
}

/// The values a function returns without `return`: the tail expressions its
/// body ends on, when its declaration names a return type
/// (`collect_tail_statements` decides tail position for the type checker
/// too). Escape analysis and the use-after-move check both read a function's
/// returned values here.
pub(crate) fn returned_tails<'b>(
    return_type: Option<&Expression>,
    body: &'b Statement,
) -> Vec<&'b Expression> {
    if !declares_a_value(return_type) {
        return Vec::new();
    }
    let mut tails = Vec::new();
    collect_tail_statements(body, &mut tails);
    (tails.into_iter())
        .filter_map(|tail| {
            if let StatementKind::Expression(value) = &tail.node {
                Some(value)
            } else {
                None
            }
        })
        .collect()
}

/// Whether a declaration's written return type names a value: a function
/// declared without one, or as returning `void`, ends on nothing it returns.
fn declares_a_value(return_type: Option<&Expression>) -> bool {
    return_type.is_some_and(|written| {
        !matches!(&written.node, ExpressionKind::Type(ty, _) if matches!(ty.kind, TypeKind::Void))
    })
}

/// What every summary of one program is computed against.
pub(crate) struct ProgramFacts<'a> {
    /// The type of every expression, by id.
    pub(crate) types: &'a HashMap<usize, Type>,
    /// Every type the program and its modules declare.
    pub(crate) type_defs: &'a HashMap<String, TypeDefinition>,
    /// The types that are resources, decided over `type_defs` at the
    /// program's instantiations.
    pub(crate) resources: &'a ResourceTypes,
    /// The function each free-function callee resolved to, by callee id.
    pub(crate) callees: &'a HashMap<usize, DeclaredFunction>,
    /// The module each top-level function declaration is made in, by
    /// statement id.
    pub(crate) modules: &'a HashMap<usize, ModuleId>,
}

/// The locals of a body, by scope: a name declared in a scope is dropped when
/// the scope ends, and a local of an enclosing scope rebound inside it may
/// name, after it, what it named before or what it was rebound to.
#[derive(Default)]
struct LocalScopes {
    locals: Locals,
    /// For each open scope, the locals in force where it opened and the
    /// names it declares.
    open: Vec<(Locals, HashSet<String>)>,
}

impl LocalScopes {
    fn enter(&mut self) {
        self.open.push((self.locals.clone(), HashSet::new()));
    }

    fn exit(&mut self) {
        let Some((mut outer, declared)) = self.open.pop() else {
            return;
        };
        for (name, named) in std::mem::take(&mut self.locals) {
            if declared.contains(&name) {
                continue;
            }
            if let Some(before) = outer.get_mut(&name) {
                before.extend(named);
            }
        }
        self.locals = outer;
    }

    /// Declares `name` in the innermost scope, naming `named`.
    fn declare(&mut self, name: &str, named: BTreeSet<ParamIndex>) {
        if let Some((_, declared)) = self.open.last_mut() {
            declared.insert(name.to_string());
        }
        self.locals.insert(name.to_string(), named);
    }

    /// Rebinds the local `name`, which names `named` from here on.
    fn rebind(&mut self, name: &str, named: BTreeSet<ParamIndex>) {
        if let Some(current) = self.locals.get_mut(name) {
            *current = named;
        }
    }
}

/// One function body's walk for the places its parameters escape to.
struct EscapeWalk<'a> {
    params: &'a [Parameter],
    facts: &'a ProgramFacts<'a>,
    summaries: &'a HashMap<FunctionId, EscapeSummary>,
    /// The ids of the tail expressions the function returns.
    tails: HashSet<usize>,
    scopes: LocalScopes,
    /// Whether the body is written in the file the program being checked is,
    /// so a report can point at the places it keeps a parameter.
    located: bool,
}

impl<'a> EscapeWalk<'a> {
    /// The value-flow rule for an expression the function hands back.
    fn analyzer(&self) -> ReturnFlowAnalyzer<'_> {
        self.analyzer_at(Keeping::Returned, None)
    }

    /// The value-flow rule for a value `keeping` keeps, at `span`.
    fn analyzer_at(&self, keeping: Keeping, span: Option<Span>) -> ReturnFlowAnalyzer<'_> {
        ReturnFlowAnalyzer {
            params: self.params,
            locals: &self.scopes.locals,
            callees: self.facts.callees,
            types: self.facts.types,
            type_definitions: self.facts.type_defs,
            resources: self.facts.resources,
            escape_summaries: self.summaries,
            keeping,
            at: span.filter(|_| self.located),
            wrapped: Cell::new(false),
            captured: Cell::new(false),
        }
    }

    /// The parameters a local bound to `value` names: those the value may
    /// be, by the value-flow rule a returned value follows — through a
    /// conditional, a match, a block, a cast or a call handing an argument
    /// back.
    fn named_by(&self, value: &Expression) -> BTreeSet<ParamIndex> {
        let mut flow = ReturnFlow::default();
        self.analyzer().classify(value, true, &mut flow);
        flow.return_aliases
    }

    /// Records every parameter the `value` returned at `span` holds.
    fn returned(&self, value: &Expression, span: Span, flow: &mut ReturnFlow) {
        (self.analyzer_at(Keeping::Returned, Some(span))).classify(value, true, flow);
    }

    /// Records every parameter the `value` stored at `span` holds: the place
    /// it is stored in keeps it, by `hop`. A store is not the return value, so
    /// it aliases nothing the caller receives.
    fn stored(&self, value: &Expression, hop: EscapeNextHop, span: Span, flow: &mut ReturnFlow) {
        let mut held = ReturnFlow::default();
        (self.analyzer_at(Keeping::Stored(hop), Some(span))).classify(value, true, &mut held);
        flow.keep_all(held);
    }

    fn statement(&mut self, stmt: &Statement, flow: &mut ReturnFlow) {
        match &stmt.node {
            StatementKind::Return(Some(e)) => self.returned(e, stmt.span, flow),
            StatementKind::Block(stmts) => self.scoped(|walk| walk.statements(stmts, flow)),
            StatementKind::Expression(e) => {
                if self.tails.contains(&e.id) {
                    self.returned(e, e.span, flow);
                }
                self.expression(e, flow);
            }
            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    let named = decl.initializer.as_ref().map(|init| {
                        self.expression(init, flow);
                        self.named_by(init)
                    });
                    self.scopes.declare(&decl.name, named.unwrap_or_default());
                }
            }
            StatementKind::If(cond, then, else_, _) => {
                self.expression(cond, flow);
                self.statement(then, flow);
                if let Some(e) = else_ {
                    self.statement(e, flow);
                }
            }
            StatementKind::While(cond, body, _) => {
                self.looped(HashSet::new(), flow, |walk, flow| {
                    walk.expression(cond, flow);
                    walk.statement(body, flow);
                })
            }
            StatementKind::For(decls, iter, body)
            | StatementKind::GpuFrame(decls, iter, body)
            | StatementKind::Forall {
                vars: decls,
                iterable: iter,
                body,
                ..
            } => {
                self.expression(iter, flow);
                let names = decls.iter().map(|decl| decl.name.clone()).collect();
                self.looped(names, flow, |walk, flow| walk.statement(body, flow));
            }
            StatementKind::GpuFrameBlock(block) => self.statement(block, flow),
            StatementKind::Return(None)
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Empty
            | StatementKind::FunctionDeclaration(_)
            | StatementKind::Use(_, _)
            | StatementKind::Type(_, _)
            | StatementKind::Enum(..)
            | StatementKind::Struct(..)
            | StatementKind::Class(_)
            | StatementKind::Trait(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..) => {}
        }
    }

    fn statements(&mut self, stmts: &[Statement], flow: &mut ReturnFlow) {
        for s in stmts {
            self.statement(s, flow);
        }
    }

    /// Walks a loop body, binding `names`, until a pass rebinds no local to
    /// a parameter it did not already name: each pass starts with the locals
    /// the last one may have left. The names a local may hold only grow and
    /// are bounded by the parameters, so the walk ends.
    fn looped(
        &mut self,
        names: HashSet<String>,
        flow: &mut ReturnFlow,
        mut pass: impl FnMut(&mut Self, &mut ReturnFlow),
    ) {
        loop {
            let before = self.scopes.locals.clone();
            self.scoped_with(names.clone(), |walk| pass(walk, flow));
            if self.scopes.locals == before {
                return;
            }
        }
    }

    /// Walks `inner` in a scope of its own.
    fn scoped(&mut self, inner: impl FnOnce(&mut Self)) {
        self.scoped_with(HashSet::new(), inner);
    }

    /// Walks `inner` in a scope of its own that binds `names`, each naming
    /// no parameter: a binding of a parameter's name hides the parameter.
    fn scoped_with(&mut self, names: HashSet<String>, inner: impl FnOnce(&mut Self)) {
        self.scopes.enter();
        for name in &names {
            self.scopes.declare(name, BTreeSet::new());
        }
        inner(self);
        self.scopes.exit();
    }

    /// Walks an expression outside the returned value for the calls that keep
    /// a parameter, the stores that hold one and the values built of one.
    fn expression(&mut self, expr: &Expression, flow: &mut ReturnFlow) {
        match &expr.node {
            ExpressionKind::Call(callee, args) => self.call(callee, args, expr.span, flow),
            ExpressionKind::Assignment(lhs, _, rhs) => self.assignment(lhs, rhs, expr.span, flow),
            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.expression(l, flow);
                self.expression(r, flow);
            }
            ExpressionKind::Unary(_, e)
            | ExpressionKind::Guard(_, e)
            | ExpressionKind::Member(e, _)
            | ExpressionKind::NamedArgument(_, e)
            | ExpressionKind::Cast(e, _) => self.expression(e, flow),
            ExpressionKind::Index(obj, idx) => {
                self.expression(obj, flow);
                self.expression(idx, flow);
            }
            ExpressionKind::Conditional(then, cond, else_, _) => {
                self.expression(cond, flow);
                self.expression(then, flow);
                self.optional(else_.as_deref(), flow);
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start, flow);
                self.optional(end.as_deref(), flow);
            }
            ExpressionKind::Block(stmts, last) => self.scoped(|walk| {
                walk.statements(stmts, flow);
                walk.expression(last, flow);
            }),
            ExpressionKind::Match(scrutinee, branches) => {
                self.match_arms(scrutinee, branches, flow)
            }
            ExpressionKind::FormattedString(elems, _) => self.each(elems, flow),
            ExpressionKind::List(elems)
            | ExpressionKind::Set(elems)
            | ExpressionKind::Tuple(elems)
            | ExpressionKind::Array(elems, _)
            | ExpressionKind::EnumValue(_, elems) => {
                self.stored(expr, EscapeNextHop::Built, expr.span, flow);
                self.each(elems, flow);
            }
            ExpressionKind::Map(pairs) => {
                self.stored(expr, EscapeNextHop::Built, expr.span, flow);
                for (k, v) in pairs {
                    self.expression(k, flow);
                    self.expression(v, flow);
                }
            }
            // A closure holds what it captures, wherever it goes.
            ExpressionKind::Lambda(_) => self.stored(expr, EscapeNextHop::Built, expr.span, flow),
            ExpressionKind::Identifier(_, _)
            | ExpressionKind::Literal(_)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Super => {}
        }
    }

    fn optional(&mut self, expr: Option<&Expression>, flow: &mut ReturnFlow) {
        if let Some(e) = expr {
            self.expression(e, flow);
        }
    }

    fn each(&mut self, exprs: &[Expression], flow: &mut ReturnFlow) {
        for e in exprs {
            self.expression(e, flow);
        }
    }

    /// Each arm is a scope binding what its patterns bind.
    fn match_arms(
        &mut self,
        scrutinee: &Expression,
        branches: &[crate::ast::MatchBranch],
        flow: &mut ReturnFlow,
    ) {
        self.expression(scrutinee, flow);
        for branch in branches {
            let mut names = HashSet::new();
            for pattern in &branch.patterns {
                collect_pattern_bindings(pattern, &mut names);
            }
            self.scoped_with(names, |walk| {
                walk.optional(branch.guard.as_deref(), flow);
                walk.statement(&branch.body, flow);
            });
        }
    }

    /// A parameter passed at a position the callee keeps is kept: a free
    /// function's or a method's, as its summary says, and every position of
    /// a call through a function-typed parameter, which this body cannot see
    /// into. A method's receiver is position 0 of its summary. A closure
    /// passed straight to a call is lent to it unless the call keeps it.
    fn call(
        &mut self,
        callee: &Expression,
        args: &[Expression],
        span: Span,
        flow: &mut ReturnFlow,
    ) {
        self.keep_passed(callee, args, span, flow);
        self.expression(callee, flow);
        for arg in args {
            if !matches!(argument_value(arg).node, ExpressionKind::Lambda(_)) {
                self.expression(arg, flow);
            }
        }
    }

    /// Records what each position the callee keeps holds, at the call's
    /// `span`; the value a constructor builds keeps every argument.
    fn keep_passed(
        &self,
        callee: &Expression,
        args: &[Expression],
        span: Span,
        flow: &mut ReturnFlow,
    ) {
        if call_builds_a_value(callee, self.facts.type_defs) {
            for arg in args {
                self.stored(arg, EscapeNextHop::Built, span, flow);
            }
            return;
        }
        let analyzer = self.analyzer();
        let Some(callee_id) = analyzer.callee_id(callee) else {
            return;
        };
        if let Some(fn_name) = analyzer.parameter_called(callee) {
            let hop = EscapeNextHop::DynamicFn {
                fn_name: fn_name.to_string(),
            };
            for arg in args {
                self.stored(arg, hop.clone(), span, flow);
            }
            return;
        }
        let (positions, summary): (Vec<&Expression>, _) =
            if let ExpressionKind::Member(receiver, method) = &callee.node {
                (
                    std::iter::once(receiver.as_ref()).chain(args).collect(),
                    analyzer.method_call_summary(receiver, method),
                )
            } else {
                (
                    args.iter().collect(),
                    self.summaries.get(&callee_id).cloned(),
                )
            };
        let Some(summary) = summary else {
            return;
        };
        for (slot, passed) in positions.into_iter().enumerate() {
            if summary.directly_escapes(slot) {
                let hop = EscapeNextHop::Call {
                    callee: callee_id.clone(),
                    param_slot: slot,
                };
                self.stored(passed, hop, span, flow);
            }
        }
    }

    /// A value assigned into a field or an element is held by the place it
    /// is stored in. A local assigned a new value names what that value
    /// names from then on.
    ///
    /// The store is recorded before the value's own parts are walked, so it
    /// is the first place a parameter stored whole is kept.
    fn assignment(
        &mut self,
        lhs: &LeftHandSideExpression,
        rhs: &Expression,
        span: Span,
        flow: &mut ReturnFlow,
    ) {
        match lhs {
            LeftHandSideExpression::Member(member) => {
                self.stored(rhs, stored_field(member), span, flow);
            }
            LeftHandSideExpression::Index(_) => {
                self.stored(rhs, EscapeNextHop::ElementStore, span, flow);
            }
            LeftHandSideExpression::Identifier(_) => {}
        }
        self.expression(rhs, flow);
        if let LeftHandSideExpression::Identifier(target) = lhs {
            if let ExpressionKind::Identifier(name, _) = &target.node {
                let named = self.named_by(rhs);
                self.scopes.rebind(name, named);
            }
        }
    }
}

/// The step a store into the member `member` takes: into a named field, or,
/// for a member written any other way, into an element.
fn stored_field(member: &Expression) -> EscapeNextHop {
    if let ExpressionKind::Member(_, field) = &member.node {
        if let ExpressionKind::Identifier(name, _) = &field.node {
            return EscapeNextHop::FieldStore {
                field: name.clone(),
            };
        }
    }
    EscapeNextHop::ElementStore
}

/// The value an argument passes, past the name it is passed under.
fn argument_value(arg: &Expression) -> &Expression {
    if let ExpressionKind::NamedArgument(_, value) = &arg.node {
        value
    } else {
        arg
    }
}

/// Adds every name `pattern` binds to `out`: an identifier, and the names
/// inside a tuple, a variant's payload or a qualified pattern.
pub(crate) fn collect_pattern_bindings(pattern: &Pattern, out: &mut HashSet<String>) {
    match pattern {
        Pattern::Identifier(name) => {
            out.insert(name.clone());
        }
        Pattern::Tuple(parts) | Pattern::EnumVariant(_, parts) => {
            for part in parts {
                collect_pattern_bindings(part, out);
            }
        }
        Pattern::Member(inner, _) => collect_pattern_bindings(inner, out),
        Pattern::Literal(_) | Pattern::Regex(_) | Pattern::Default => {}
    }
}
