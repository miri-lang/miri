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
//! Functions are keyed by their *qualified name*: a plain name for free
//! functions (e.g. `"save"`) and `ClassName_method` for methods (e.g.
//! `"Cache_store"`).  This matches the mangling convention used throughout
//! MIR lowering.
//!
//! # FFI summaries
//!
//! Escape summaries for `runtime "core" fn` declarations (FFI-only, no body)
//! are hand-authored in `src/runtime/core/escape_summaries.toml` and loaded
//! at startup via [`load_ffi_summaries`].  The TOML is embedded into the
//! compiler binary with `include_str!`.
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
//!   by [`super::utils::is_resource`], which descends into a generic
//!   parameter's constraint when classifying it.
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

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Deserialize;

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::ast::Parameter;

use super::context::TypeDefinition;
use super::utils::is_auto_copy;

/// Zero-based index of a parameter in a function's parameter list.
pub type ParamIndex = usize;

/// Qualified function name used as the key in [`super::context::Context::escape_summaries`].
///
/// - Free function `save` → `"save"`
/// - Method `Cache::store` → `"Cache_store"`
pub type FunctionId = String;

/// Describes a single conditional escape: parameter `param` escapes iff the
/// fn-typed parameter at `via_fn_param` has `callee_param` in its own escape
/// set at the call site where it is resolved.
///
/// Read as: *"my parameter `param` escapes through whichever function is
/// passed as argument `via_fn_param`, specifically through that function's
/// parameter slot `callee_param`."*
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConditionalEscape {
    /// Which managed-type parameter of this function flows through a fn-typed arg.
    pub param: ParamIndex,
    /// Which fn-typed parameter carries the escape (index into the same param list).
    pub via_fn_param: ParamIndex,
    /// Which parameter slot of the fn-typed callee `param` is bound to.
    pub callee_param: ParamIndex,
}

/// One step in an escape chain — describes what happens to a parameter at the
/// function boundary where it first escapes.
///
/// Stored in [`EscapeSummary::escape_next_hops`] and followed at error-report
/// time to build the full "consumed because" chain shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeNextHop {
    /// The parameter is passed to `callee` at `param_slot`, and that callee
    /// escapes `param_slot`.  Follow the callee's summary to continue the chain.
    Call { callee: String, param_slot: usize },
    /// The parameter is returned directly from this function.
    Return,
    /// The parameter flows into an aggregate (list, tuple, struct constructor)
    /// that is returned from this function.
    ReturnAggregate,
    /// The parameter is assigned into a field of `self` — it escapes into the
    /// heap via `self.<field>`.
    FieldStore { field: String },
    /// The parameter is captured by a lambda that this function returns.
    ClosureCapture,
    /// The parameter is passed to a dynamic fn-value (fn-typed parameter or
    /// local); all managed args are conservatively treated as escaping.
    DynamicFn { fn_name: String },
}

/// Escape summary for a single function.
///
/// Computed bottom-up over the call graph by the escape analysis pass
/// (`src/type_checker/escape_analysis.rs`).  Hand-authored entries
/// for FFI-only declarations live in `src/runtime/core/escape_summaries.toml`.
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
    /// For each param in `direct_escapes`, the first hop that explains WHY it
    /// escapes.  Populated in a post-fixpoint pass by [`resolve_next_hops`];
    /// absent during the fixpoint itself (always empty then, so PartialEq is
    /// unaffected by this field during iteration).
    pub escape_next_hops: HashMap<ParamIndex, EscapeNextHop>,
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
///    - Managed element type → both sets contain `p`.
///    - Auto-copy element type → neither set contains `p` (indexing copies).
/// 4. `return p.field` — same split based on the member access's result type.
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
    let analyzer = ReturnFlowAnalyzer {
        params,
        types,
        type_definitions,
        escape_summaries,
    };
    let mut flow = ReturnFlow::default();
    analyzer.classify(return_expr, true, &mut flow);
    flow
}

struct ReturnFlowAnalyzer<'a> {
    params: &'a [Parameter],
    types: &'a HashMap<usize, Type>,
    type_definitions: &'a HashMap<String, TypeDefinition>,
    escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
}

impl<'a> ReturnFlowAnalyzer<'a> {
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
            | ExpressionKind::Set(elems) => {
                self.classify_list_tuple_set(elems, aliases_return, flow);
            }
            ExpressionKind::Array(elems, _) => {
                self.classify_array(elems, aliases_return, flow);
            }
            ExpressionKind::Map(pairs) => {
                self.classify_map(pairs, aliases_return, flow);
            }
            ExpressionKind::EnumValue(_, values) => {
                self.classify_enum_value(values, aliases_return, flow);
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
                self.classify_range(start, end, flow);
            }
            ExpressionKind::FormattedString(parts) => {
                self.classify_formatted_string(parts, flow);
            }
            ExpressionKind::Assignment(_, _, rhs) => {
                self.classify(rhs, aliases_return, flow);
            }
            ExpressionKind::Lambda(lambda_data) => {
                if aliases_return {
                    self.scan_lambda_captures(&lambda_data.body, flow);
                }
            }
            ExpressionKind::Cast(value_expr, _target_type_expr) => {
                self.classify(value_expr, aliases_return, flow);
            }
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
        if let Some(idx) = self.param_index(name) {
            if aliases_return && self.is_managed_expr(expr) {
                flow.direct_escapes.insert(idx);
                flow.return_aliases.insert(idx);
            }
        }
    }

    fn classify_list_tuple_set(
        &self,
        elems: &[Expression],
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        for e in elems {
            self.classify(e, aliases_return, flow);
        }
    }

    fn classify_array(&self, elems: &[Expression], aliases_return: bool, flow: &mut ReturnFlow) {
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

    fn classify_enum_value(
        &self,
        values: &[Expression],
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        for v in values {
            self.classify(v, aliases_return, flow);
        }
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
        self.classify(obj, alias_through, flow);
        self.classify(idx_expr, false, flow);
    }

    fn classify_member(
        &self,
        obj: &Expression,
        aliases_return: bool,
        expr: &Expression,
        flow: &mut ReturnFlow,
    ) {
        let alias_through = aliases_return && self.is_managed_expr(expr);
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

    fn classify_range(
        &self,
        start: &Expression,
        end: &Option<Box<Expression>>,
        flow: &mut ReturnFlow,
    ) {
        self.classify(start, false, flow);
        if let Some(e) = end {
            self.classify(e, false, flow);
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
            ExpressionKind::Identifier(name, _) => {
                if let Some(idx) = self.param_index(name) {
                    if self.is_managed_expr(expr) {
                        flow.direct_escapes.insert(idx);
                        flow.return_aliases.insert(idx);
                    }
                }
            }
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
            ExpressionKind::FormattedString(parts) => {
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

        if let Some(s) = summary {
            if let Some(obj) = receiver {
                self.apply_summary_for_arg(obj, 0, s, aliases_return, flow);
            }
            for (i, arg) in args.iter().enumerate() {
                let summary_idx = i + arg_offset;
                self.apply_summary_for_arg(arg, summary_idx, s, aliases_return, flow);
            }
        } else {
            // No summary available — this analyzer alone cannot decide whether
            // the call propagates flow.  The escape analysis pass handles
            // the conservative fallback ("every managed param escapes") for
            // unresolved callees.  For value-flow purposes, we
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
        aliases_return: bool,
        flow: &mut ReturnFlow,
    ) {
        // Rule 5: callee consumes this slot — mark the leaf identifier (if
        // a managed param) as direct_escape regardless of return-alias state.
        if summary.direct_escapes.contains(&summary_idx) {
            self.consume_leaf_identifier(arg, flow);
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
    /// to a managed-typed parameter, mark that parameter as `direct_escapes`.
    fn consume_leaf_identifier(&self, arg: &Expression, flow: &mut ReturnFlow) {
        let inner = match &arg.node {
            ExpressionKind::NamedArgument(_, val) => val.as_ref(),
            _ => arg,
        };
        if let ExpressionKind::Identifier(name, _) = &inner.node {
            if let Some(idx) = self.param_index(name) {
                if self.is_managed_expr(inner) {
                    flow.direct_escapes.insert(idx);
                }
            }
        }
    }

    /// Maps a parameter name back to its zero-based index, if it is one of the
    /// function's parameters.  `self` is index 0 for methods (mirroring the
    /// mangling convention used elsewhere); free functions use the explicit
    /// parameter list.
    fn param_index(&self, name: &str) -> Option<ParamIndex> {
        self.params.iter().position(|p| p.name == name)
    }

    /// Returns `true` if `expr`'s resolved type is managed (non-auto-copy,
    /// heap-allocated).  Auto-copy expressions (primitives, small POD structs)
    /// produce by-value results that cannot alias another heap.
    fn is_managed_expr(&self, expr: &Expression) -> bool {
        match self.types.get(&expr.id) {
            Some(ty) => !is_auto_copy(&ty.kind, self.type_definitions),
            None => false,
        }
    }

    /// Looks up the escape summary for `callee` if it is a free-function
    /// identifier or a direct (non-virtual) method call on a concrete class.
    ///
    /// Inheritance walks for inherited methods and trait/abstract joins live
    /// elsewhere and need the type definitions.  This analyzer
    /// focuses on the literal-callee cases (`return f(p)`); virtual joining
    /// is folded in by the broader escape analysis when it consults this analyzer.
    fn resolve_callee_summary(&self, callee: &Expression) -> Option<EscapeSummary> {
        match &callee.node {
            ExpressionKind::Identifier(name, _) => {
                self.escape_summaries.get(name.as_str()).cloned()
            }
            ExpressionKind::Member(obj, method_expr) => {
                let ExpressionKind::Identifier(method_name, _) = &method_expr.node else {
                    return None;
                };
                let receiver_ty = self.types.get(&obj.id)?;
                let TypeKind::Custom(type_name, _) = &receiver_ty.kind else {
                    return None;
                };
                let key = format!("{type_name}_{method_name}");
                self.escape_summaries.get(&key).cloned()
            }
            _ => None,
        }
    }
}

/// Serde-friendly intermediate form for one entry in `escape_summaries.toml`.
/// Uses `Vec` rather than `BTreeSet` because TOML arrays map naturally to Vec.
#[derive(Debug, Deserialize)]
struct TomlSummaryEntry {
    #[serde(default)]
    direct_escapes: Vec<ParamIndex>,
    #[serde(default)]
    return_aliases: Vec<ParamIndex>,
    #[serde(default)]
    conditional_escapes: Vec<ConditionalEscape>,
}

impl From<TomlSummaryEntry> for EscapeSummary {
    fn from(e: TomlSummaryEntry) -> Self {
        EscapeSummary {
            direct_escapes: e.direct_escapes.into_iter().collect(),
            conditional_escapes: e.conditional_escapes,
            return_aliases: e.return_aliases.into_iter().collect(),
            escape_next_hops: HashMap::new(),
        }
    }
}

/// Loads the hand-authored FFI escape summaries from the embedded TOML sidecar
/// (`src/runtime/core/escape_summaries.toml`).
///
/// The TOML is embedded at compile time; no filesystem access occurs at
/// runtime.  Returns an empty map if parsing fails (panics via `debug_assert!`
/// in debug builds, silent in release) — this is safe because the escape
/// analysis falls back to the conservative "all managed params escape" default
/// for any unknown function.  In practice the embedded TOML is validated by
/// `load_ffi_summaries_parses_without_panic` before release.
pub fn load_ffi_summaries() -> HashMap<FunctionId, EscapeSummary> {
    const TOML_SRC: &str = include_str!("../runtime/core/escape_summaries.toml");

    match toml::from_str::<HashMap<FunctionId, TomlSummaryEntry>>(TOML_SRC) {
        Ok(raw) => raw.into_iter().map(|(k, v)| (k, v.into())).collect(),
        Err(e) => {
            // Unreachable in normal builds — the TOML is embedded and tested.
            // In debug mode, surface the error so it is caught early.
            debug_assert!(false, "failed to parse escape_summaries.toml: {e}");
            HashMap::new()
        }
    }
}

/// Captures a function's parameter list (with `self` at index 0 for methods)
/// and a reference to its body.
struct FunctionDef<'a> {
    params: Vec<Parameter>,
    body: &'a Statement,
}

/// Computes escape summaries for every user-defined function in `stmts`,
/// merges them with the pre-loaded `ffi_summaries`, and returns the combined
/// map.  Analysis is bottom-up over the call graph (Tarjan SCC + fixpoint
/// within each SCC).
///
/// Only user functions whose bodies are visible in `stmts` are analysed.
/// Stdlib declarations are represented by their FFI summaries in `ffi_summaries`.
pub fn compute_escape_summaries(
    stmts: &[Statement],
    types: &HashMap<usize, Type>,
    type_definitions: &HashMap<String, super::context::TypeDefinition>,
    ffi_summaries: HashMap<FunctionId, EscapeSummary>,
) -> HashMap<FunctionId, EscapeSummary> {
    let mut summaries = ffi_summaries;

    // Step 1: collect all user function definitions.
    let mut fn_defs: HashMap<FunctionId, FunctionDef<'_>> = HashMap::new();
    collect_function_defs(stmts, &mut fn_defs, None);

    if fn_defs.is_empty() {
        return summaries;
    }

    // Step 2: build a call graph over known user functions.
    let call_graph = build_call_graph(&fn_defs, types);

    // Step 3: Tarjan SCC — returns SCCs with leaf SCCs first (bottom-up order).
    let fn_ids: Vec<FunctionId> = fn_defs.keys().cloned().collect();
    let sccs = tarjan_sccs(&fn_ids, &call_graph);

    // Step 4: process each SCC in order, fixpointing within the SCC.
    for scc in &sccs {
        loop {
            let mut changed = false;
            for fn_id in scc {
                let Some(def) = fn_defs.get(fn_id.as_str()) else {
                    continue;
                };
                let new_summary =
                    compute_one_summary(&def.params, def.body, types, type_definitions, &summaries);
                let old = summaries.get(fn_id.as_str()).cloned().unwrap_or_default();
                if new_summary != old {
                    summaries.insert(fn_id.clone(), new_summary);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    // Step 5: post-fixpoint pass — compute escape chains for all user functions.
    // Processed in bottom-up SCC order so callee chains are ready before callers.
    let all_hops: Vec<(FunctionId, HashMap<ParamIndex, EscapeNextHop>)> = sccs
        .iter()
        .flat_map(|scc| scc.iter())
        .filter_map(|fn_id| {
            let def = fn_defs.get(fn_id.as_str())?;
            let summary = summaries.get(fn_id.as_str())?;
            if summary.direct_escapes.is_empty() {
                return None;
            }
            let hops = resolve_next_hops(
                &def.params,
                &summary.direct_escapes,
                def.body,
                &summaries,
                types,
            );
            Some((fn_id.clone(), hops))
        })
        .collect();

    for (fn_id, hops) in all_hops {
        if let Some(summary) = summaries.get_mut(&fn_id) {
            summary.escape_next_hops = hops;
        }
    }

    summaries
}

fn collect_function_defs<'a>(
    stmts: &'a [Statement],
    fn_defs: &mut HashMap<FunctionId, FunctionDef<'a>>,
    class_name: Option<&str>,
) {
    for stmt in stmts {
        match &stmt.node {
            StatementKind::FunctionDeclaration(decl) => {
                let Some(body) = decl.body.as_deref() else {
                    continue;
                };
                let fn_id = match class_name {
                    Some(cls) => format!("{cls}_{}", decl.name),
                    None => decl.name.clone(),
                };
                let params = match class_name {
                    Some(_) => {
                        let mut p = vec![synthetic_self_param()];
                        p.extend_from_slice(&decl.params);
                        p
                    }
                    None => decl.params.clone(),
                };
                fn_defs.insert(fn_id, FunctionDef { params, body });
            }
            StatementKind::Class(cd) => {
                if let Some(cls) = extract_name_from_expr(&cd.name) {
                    collect_function_defs(&cd.body, fn_defs, Some(&cls.clone()));
                }
            }
            StatementKind::Struct(name_expr, _, _, methods, _) => {
                if let Some(s) = extract_name_from_expr(name_expr) {
                    collect_function_defs(methods, fn_defs, Some(&s.clone()));
                }
            }
            StatementKind::Block(stmts) => {
                collect_function_defs(stmts, fn_defs, class_name);
            }
            _ => {}
        }
    }
}

/// Creates a synthetic `self` parameter used as index-0 in method param lists.
fn synthetic_self_param() -> Parameter {
    use crate::ast::factory::{expr_with_span, make_type};
    use crate::error::syntax::Span;
    Parameter {
        name: "self".to_string(),
        typ: Box::new(expr_with_span(
            ExpressionKind::Type(Box::new(make_type(TypeKind::Void)), false),
            Span::new(0, 0),
        )),
        guard: None,
        default_value: None,
        is_out: false,
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

/// Builds a call graph over the known user function IDs.  An edge `f → g`
/// exists when function `f`'s body contains a call that resolves to `g`.
/// Only edges to other user functions (present in `fn_defs`) are recorded;
/// edges to FFI or unknown functions are not needed here (their summaries are
/// already in the `summaries` map and consulted directly during summary
/// computation).
fn build_call_graph<'a>(
    fn_defs: &HashMap<FunctionId, FunctionDef<'a>>,
    types: &HashMap<usize, Type>,
) -> HashMap<FunctionId, Vec<FunctionId>> {
    let known: HashSet<&str> = fn_defs.keys().map(String::as_str).collect();
    let mut graph: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();

    for (fn_id, def) in fn_defs {
        let mut callees: Vec<FunctionId> = Vec::new();
        collect_callees_from_stmt(def.body, types, &known, &mut callees);
        // Deduplicate (a callee may appear multiple times).
        callees.sort_unstable();
        callees.dedup();
        graph.insert(fn_id.clone(), callees);
    }

    graph
}

fn collect_callees_from_stmt(
    stmt: &Statement,
    types: &HashMap<usize, Type>,
    known: &HashSet<&str>,
    out: &mut Vec<FunctionId>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                collect_callees_from_stmt(s, types, known, out);
            }
        }
        StatementKind::Expression(e) => {
            collect_callees_from_expr(e, types, known, out);
        }
        StatementKind::Return(Some(e)) => {
            collect_callees_from_expr(e, types, known, out);
        }
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    collect_callees_from_expr(init, types, known, out);
                }
            }
        }
        StatementKind::If(cond, then, else_, _) => {
            collect_callees_from_expr(cond, types, known, out);
            collect_callees_from_stmt(then, types, known, out);
            if let Some(e) = else_ {
                collect_callees_from_stmt(e, types, known, out);
            }
        }
        StatementKind::While(cond, body, _) => {
            collect_callees_from_expr(cond, types, known, out);
            collect_callees_from_stmt(body, types, known, out);
        }
        StatementKind::For(_, iter, body) | StatementKind::GpuFrame(_, iter, body) => {
            collect_callees_from_expr(iter, types, known, out);
            collect_callees_from_stmt(body, types, known, out);
        }
        StatementKind::Forall {
            iterable: iter,
            body,
            ..
        } => {
            collect_callees_from_expr(iter, types, known, out);
            collect_callees_from_stmt(body, types, known, out);
        }
        StatementKind::FunctionDeclaration(_) => {}
        _ => {}
    }
}

fn collect_callees_from_expr(
    expr: &Expression,
    types: &HashMap<usize, Type>,
    known: &HashSet<&str>,
    out: &mut Vec<FunctionId>,
) {
    match &expr.node {
        ExpressionKind::Call(callee, args) => {
            collect_callees_call(callee, args, types, known, out);
        }
        ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
            collect_callees_from_expr(l, types, known, out);
            collect_callees_from_expr(r, types, known, out);
        }
        ExpressionKind::Unary(_, e) | ExpressionKind::Guard(_, e) => {
            collect_callees_from_expr(e, types, known, out);
        }
        ExpressionKind::Index(obj, idx) => {
            collect_callees_from_expr(obj, types, known, out);
            collect_callees_from_expr(idx, types, known, out);
        }
        ExpressionKind::Member(obj, _) => collect_callees_from_expr(obj, types, known, out),
        ExpressionKind::Conditional(then, cond, else_, _) => {
            collect_callees_from_expr(cond, types, known, out);
            collect_callees_from_expr(then, types, known, out);
            if let Some(e) = else_ {
                collect_callees_from_expr(e, types, known, out);
            }
        }
        ExpressionKind::Block(stmts, e) => {
            for s in stmts {
                collect_callees_from_stmt(s, types, known, out);
            }
            collect_callees_from_expr(e, types, known, out);
        }
        ExpressionKind::Match(sc, branches) => {
            collect_callees_from_expr(sc, types, known, out);
            for b in branches {
                if let Some(g) = &b.guard {
                    collect_callees_from_expr(g, types, known, out);
                }
                collect_callees_from_stmt(&b.body, types, known, out);
            }
        }
        ExpressionKind::List(elems) | ExpressionKind::Set(elems) | ExpressionKind::Tuple(elems) => {
            collect_callees_from_elems(elems, types, known, out);
        }
        ExpressionKind::Array(elems, _) => {
            collect_callees_from_elems(elems, types, known, out);
        }
        ExpressionKind::Map(pairs) => {
            collect_callees_from_pairs(pairs, types, known, out);
        }
        ExpressionKind::EnumValue(_, vals) => {
            collect_callees_from_elems(vals, types, known, out);
        }
        ExpressionKind::Assignment(_, _, rhs) => collect_callees_from_expr(rhs, types, known, out),
        ExpressionKind::NamedArgument(_, val) => collect_callees_from_expr(val, types, known, out),
        ExpressionKind::FormattedString(parts) => {
            collect_callees_from_elems(parts, types, known, out);
        }
        ExpressionKind::Range(start, end_, _) => {
            collect_callees_from_expr(start, types, known, out);
            if let Some(e) = end_ {
                collect_callees_from_expr(e, types, known, out);
            }
        }
        ExpressionKind::Cast(value_expr, _target_type_expr) => {
            collect_callees_from_expr(value_expr, types, known, out);
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
    types: &HashMap<usize, Type>,
    known: &HashSet<&str>,
    out: &mut Vec<FunctionId>,
) {
    for e in elems {
        collect_callees_from_expr(e, types, known, out);
    }
}

fn collect_callees_from_pairs(
    pairs: &[(Expression, Expression)],
    types: &HashMap<usize, Type>,
    known: &HashSet<&str>,
    out: &mut Vec<FunctionId>,
) {
    for (k, v) in pairs {
        collect_callees_from_expr(k, types, known, out);
        collect_callees_from_expr(v, types, known, out);
    }
}

fn collect_callees_call(
    callee: &Expression,
    args: &[Expression],
    types: &HashMap<usize, Type>,
    known: &HashSet<&str>,
    out: &mut Vec<FunctionId>,
) {
    match &callee.node {
        ExpressionKind::Identifier(name, _) if known.contains(name.as_str()) => {
            out.push(name.clone());
        }
        ExpressionKind::Member(obj, method_expr) => {
            if let ExpressionKind::Identifier(method, _) = &method_expr.node {
                if let Some(ty) = types.get(&obj.id) {
                    if let TypeKind::Custom(class, _) = &ty.kind {
                        let key = format!("{class}_{method}");
                        if known.contains(key.as_str()) {
                            out.push(key);
                        }
                    }
                }
            }
            collect_callees_from_expr(obj, types, known, out);
        }
        _ => {
            collect_callees_from_expr(callee, types, known, out);
        }
    }
    for a in args {
        collect_callees_from_expr(a, types, known, out);
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
fn compute_one_summary(
    params: &[Parameter],
    body: &Statement,
    types: &HashMap<usize, Type>,
    type_defs: &HashMap<String, super::context::TypeDefinition>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
) -> EscapeSummary {
    let mut direct_escapes = BTreeSet::new();
    let mut return_aliases = BTreeSet::new();

    walk_stmt_for_escapes(
        body,
        params,
        types,
        type_defs,
        summaries,
        &mut direct_escapes,
        &mut return_aliases,
    );

    EscapeSummary {
        direct_escapes,
        conditional_escapes: vec![],
        return_aliases,
        escape_next_hops: HashMap::new(),
    }
}

/// Walks a statement recursively, accumulating escape evidence.
///
/// - `Return(Some(e))` → delegates to `analyze_return_value` (rules 1–7).
/// - Other statements → recurses and calls `walk_expr_for_rule4` on
///   expressions that appear in non-return position (rule 4: free-function
///   call sites whose callee summary marks an argument as escaping).
fn walk_stmt_for_escapes(
    stmt: &Statement,
    params: &[Parameter],
    types: &HashMap<usize, Type>,
    type_defs: &HashMap<String, super::context::TypeDefinition>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
    return_aliases: &mut BTreeSet<ParamIndex>,
) {
    match &stmt.node {
        StatementKind::Return(Some(e)) => {
            let flow = analyze_return_value(e, params, types, type_defs, summaries);
            direct_escapes.extend(flow.direct_escapes);
            return_aliases.extend(flow.return_aliases);
        }
        StatementKind::Return(None)
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Empty => {}
        StatementKind::Block(stmts) => {
            walk_block_for_escapes(
                stmts,
                params,
                types,
                type_defs,
                summaries,
                direct_escapes,
                return_aliases,
            );
        }
        StatementKind::Expression(e) => {
            walk_expr_for_rule4(e, params, summaries, direct_escapes);
        }
        StatementKind::Variable(decls, _) => {
            walk_var_decls_for_rule4(decls, params, summaries, direct_escapes);
        }
        StatementKind::If(cond, then, else_, _) => {
            walk_if_for_escapes(
                cond,
                then,
                else_,
                params,
                types,
                type_defs,
                summaries,
                direct_escapes,
                return_aliases,
            );
        }
        StatementKind::While(cond, body, _) => {
            walk_while_for_escapes(
                cond,
                body,
                params,
                types,
                type_defs,
                summaries,
                direct_escapes,
                return_aliases,
            );
        }
        StatementKind::For(_, iter, body) | StatementKind::GpuFrame(_, iter, body) => {
            walk_for_for_escapes(
                iter,
                body,
                params,
                types,
                type_defs,
                summaries,
                direct_escapes,
                return_aliases,
            );
        }
        StatementKind::Forall {
            iterable: iter,
            body,
            ..
        } => {
            walk_for_for_escapes(
                iter,
                body,
                params,
                types,
                type_defs,
                summaries,
                direct_escapes,
                return_aliases,
            );
        }
        StatementKind::GpuFrameBlock(block) => {
            walk_stmt_for_escapes(
                block,
                params,
                types,
                type_defs,
                summaries,
                direct_escapes,
                return_aliases,
            );
        }
        StatementKind::FunctionDeclaration(_) => {}
        StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
    }
}

fn walk_block_for_escapes(
    stmts: &[Statement],
    params: &[Parameter],
    types: &HashMap<usize, Type>,
    type_defs: &HashMap<String, super::context::TypeDefinition>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
    return_aliases: &mut BTreeSet<ParamIndex>,
) {
    for s in stmts {
        walk_stmt_for_escapes(
            s,
            params,
            types,
            type_defs,
            summaries,
            direct_escapes,
            return_aliases,
        );
    }
}

fn walk_var_decls_for_rule4(
    decls: &[crate::ast::VariableDeclaration],
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    for d in decls {
        if let Some(init) = &d.initializer {
            walk_expr_for_rule4(init, params, summaries, direct_escapes);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_if_for_escapes(
    cond: &Expression,
    then: &Statement,
    else_: &Option<Box<Statement>>,
    params: &[Parameter],
    types: &HashMap<usize, Type>,
    type_defs: &HashMap<String, super::context::TypeDefinition>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
    return_aliases: &mut BTreeSet<ParamIndex>,
) {
    walk_expr_for_rule4(cond, params, summaries, direct_escapes);
    walk_stmt_for_escapes(
        then,
        params,
        types,
        type_defs,
        summaries,
        direct_escapes,
        return_aliases,
    );
    if let Some(e) = else_ {
        walk_stmt_for_escapes(
            e,
            params,
            types,
            type_defs,
            summaries,
            direct_escapes,
            return_aliases,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_while_for_escapes(
    cond: &Expression,
    body: &Statement,
    params: &[Parameter],
    types: &HashMap<usize, Type>,
    type_defs: &HashMap<String, super::context::TypeDefinition>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
    return_aliases: &mut BTreeSet<ParamIndex>,
) {
    walk_expr_for_rule4(cond, params, summaries, direct_escapes);
    walk_stmt_for_escapes(
        body,
        params,
        types,
        type_defs,
        summaries,
        direct_escapes,
        return_aliases,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_for_for_escapes(
    iter: &Expression,
    body: &Statement,
    params: &[Parameter],
    types: &HashMap<usize, Type>,
    type_defs: &HashMap<String, super::context::TypeDefinition>,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
    return_aliases: &mut BTreeSet<ParamIndex>,
) {
    walk_expr_for_rule4(iter, params, summaries, direct_escapes);
    walk_stmt_for_escapes(
        body,
        params,
        types,
        type_defs,
        summaries,
        direct_escapes,
        return_aliases,
    );
}

/// Statement-level companion to [`walk_expr_for_rule4`]: visits every
/// expression sub-statement that could contain a rule-4 call site.  Used to
/// walk match-branch bodies and block-expression statement lists, which are
/// statements rather than expressions.
fn walk_stmt_for_rule4(
    stmt: &Statement,
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                walk_stmt_for_rule4(s, params, summaries, direct_escapes);
            }
        }
        StatementKind::Expression(e) => {
            walk_expr_for_rule4(e, params, summaries, direct_escapes);
        }
        StatementKind::Return(Some(e)) => {
            walk_expr_for_rule4(e, params, summaries, direct_escapes);
        }
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    walk_expr_for_rule4(init, params, summaries, direct_escapes);
                }
            }
        }
        StatementKind::If(cond, then, else_, _) => {
            walk_expr_for_rule4(cond, params, summaries, direct_escapes);
            walk_stmt_for_rule4(then, params, summaries, direct_escapes);
            if let Some(e) = else_ {
                walk_stmt_for_rule4(e, params, summaries, direct_escapes);
            }
        }
        StatementKind::While(cond, body, _) => {
            walk_expr_for_rule4(cond, params, summaries, direct_escapes);
            walk_stmt_for_rule4(body, params, summaries, direct_escapes);
        }
        StatementKind::For(_, iter, body) | StatementKind::GpuFrame(_, iter, body) => {
            walk_expr_for_rule4(iter, params, summaries, direct_escapes);
            walk_stmt_for_rule4(body, params, summaries, direct_escapes);
        }
        StatementKind::Forall {
            iterable: iter,
            body,
            ..
        } => {
            walk_expr_for_rule4(iter, params, summaries, direct_escapes);
            walk_stmt_for_rule4(body, params, summaries, direct_escapes);
        }
        _ => {}
    }
}

/// Walks an expression for rule-4 escape evidence: when the expression
/// contains a call to a **free function** (not a method call) whose escape
/// summary marks argument slot `i` as `directly_escapes`, and the argument at
/// slot `i` is an identifier naming one of our parameters, we add that
/// parameter's index to `direct_escapes`.
///
/// Method calls are intentionally excluded to avoid false positives in cases
/// where the receiver is a local variable that does not itself escape the
/// function (e.g. `trash.push(items)` where `trash` is a local list).
fn walk_expr_for_rule4(
    expr: &Expression,
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    match &expr.node {
        ExpressionKind::Call(callee, args) => {
            walk_call_for_rule4(callee, args, params, summaries, direct_escapes);
        }
        ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
            walk_expr_for_rule4(l, params, summaries, direct_escapes);
            walk_expr_for_rule4(r, params, summaries, direct_escapes);
        }
        ExpressionKind::Unary(_, e) | ExpressionKind::Guard(_, e) => {
            walk_expr_for_rule4(e, params, summaries, direct_escapes);
        }
        ExpressionKind::Index(obj, idx) => {
            walk_expr_for_rule4(obj, params, summaries, direct_escapes);
            walk_expr_for_rule4(idx, params, summaries, direct_escapes);
        }
        ExpressionKind::Member(obj, _) => {
            walk_expr_for_rule4(obj, params, summaries, direct_escapes);
        }
        ExpressionKind::Conditional(then, cond, else_, _) => {
            walk_conditional_for_rule4(cond, then, else_, params, summaries, direct_escapes);
        }
        ExpressionKind::Block(stmts, e) => {
            walk_block_for_rule4(stmts, e, params, summaries, direct_escapes);
        }
        ExpressionKind::Match(sc, branches) => {
            walk_match_for_rule4(sc, branches, params, summaries, direct_escapes);
        }
        ExpressionKind::List(elems) | ExpressionKind::Set(elems) | ExpressionKind::Tuple(elems) => {
            walk_elems_for_rule4(elems, params, summaries, direct_escapes);
        }
        ExpressionKind::Array(elems, _) => {
            walk_elems_for_rule4(elems, params, summaries, direct_escapes);
        }
        ExpressionKind::Map(pairs) => {
            walk_pairs_for_rule4(pairs, params, summaries, direct_escapes);
        }
        ExpressionKind::EnumValue(_, vals) => {
            walk_elems_for_rule4(vals, params, summaries, direct_escapes);
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            walk_assignment_for_rule4(lhs, rhs, params, summaries, direct_escapes);
        }
        ExpressionKind::NamedArgument(_, val) => {
            walk_expr_for_rule4(val, params, summaries, direct_escapes);
        }
        ExpressionKind::FormattedString(parts) => {
            walk_elems_for_rule4(parts, params, summaries, direct_escapes);
        }
        ExpressionKind::Range(start, end_, _) => {
            walk_expr_for_rule4(start, params, summaries, direct_escapes);
            if let Some(e) = end_ {
                walk_expr_for_rule4(e, params, summaries, direct_escapes);
            }
        }
        ExpressionKind::Cast(value_expr, _target_type_expr) => {
            walk_expr_for_rule4(value_expr, params, summaries, direct_escapes);
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

fn walk_call_for_rule4(
    callee: &Expression,
    args: &[Expression],
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    if let ExpressionKind::Identifier(name, _) = &callee.node {
        if let Some(summary) = summaries.get(name.as_str()) {
            for (i, arg) in args.iter().enumerate() {
                if summary.directly_escapes(i) {
                    apply_rule4_to_arg(arg, i, params, direct_escapes);
                }
            }
        }
    }
    walk_expr_for_rule4(callee, params, summaries, direct_escapes);
    for a in args {
        walk_expr_for_rule4(a, params, summaries, direct_escapes);
    }
}

fn walk_conditional_for_rule4(
    cond: &Expression,
    then: &Expression,
    else_: &Option<Box<Expression>>,
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    walk_expr_for_rule4(cond, params, summaries, direct_escapes);
    walk_expr_for_rule4(then, params, summaries, direct_escapes);
    if let Some(e) = else_ {
        walk_expr_for_rule4(e, params, summaries, direct_escapes);
    }
}

fn walk_block_for_rule4(
    stmts: &[Statement],
    e: &Expression,
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    for s in stmts {
        walk_stmt_for_rule4(s, params, summaries, direct_escapes);
    }
    walk_expr_for_rule4(e, params, summaries, direct_escapes);
}

fn walk_match_for_rule4(
    sc: &Expression,
    branches: &[crate::ast::MatchBranch],
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    walk_expr_for_rule4(sc, params, summaries, direct_escapes);
    for b in branches {
        if let Some(g) = &b.guard {
            walk_expr_for_rule4(g, params, summaries, direct_escapes);
        }
        walk_stmt_for_rule4(b.body.as_ref(), params, summaries, direct_escapes);
    }
}

fn walk_elems_for_rule4(
    elems: &[Expression],
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    for e in elems {
        walk_expr_for_rule4(e, params, summaries, direct_escapes);
    }
}

fn walk_pairs_for_rule4(
    pairs: &[(Expression, Expression)],
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    for (k, v) in pairs {
        walk_expr_for_rule4(k, params, summaries, direct_escapes);
        walk_expr_for_rule4(v, params, summaries, direct_escapes);
    }
}

fn walk_assignment_for_rule4(
    lhs: &LeftHandSideExpression,
    rhs: &Expression,
    params: &[Parameter],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    if let LeftHandSideExpression::Member(member_expr) = lhs {
        if let ExpressionKind::Member(obj, _) = &member_expr.node {
            if let ExpressionKind::Identifier(obj_name, _) = &obj.node {
                if obj_name == "self" {
                    apply_rule4_to_arg(rhs, 0, params, direct_escapes);
                }
            }
        }
    }
    walk_expr_for_rule4(rhs, params, summaries, direct_escapes);
}

/// If `arg` (or its `NamedArgument` wrapper) is a bare identifier that names
/// one of `params`, adds that parameter's index to `direct_escapes`.
fn apply_rule4_to_arg(
    arg: &Expression,
    _summary_idx: ParamIndex,
    params: &[Parameter],
    direct_escapes: &mut BTreeSet<ParamIndex>,
) {
    let inner = match &arg.node {
        ExpressionKind::NamedArgument(_, val) => val.as_ref(),
        _ => arg,
    };
    if let ExpressionKind::Identifier(name, _) = &inner.node {
        if let Some(idx) = params.iter().position(|p| p.name == *name) {
            direct_escapes.insert(idx);
        }
    }
}

/// For each param in `direct_escapes`, finds the first [`EscapeNextHop`] by
/// walking the function body.  Returns a map from param index to hop.
///
/// Must be called after the fixpoint has converged so that callee summaries
/// (consulted when the hop is a `Call`) are stable.
fn resolve_next_hops(
    params: &[Parameter],
    direct_escapes: &BTreeSet<ParamIndex>,
    body: &Statement,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    types: &HashMap<usize, Type>,
) -> HashMap<ParamIndex, EscapeNextHop> {
    let mut hops = HashMap::new();
    for &param_idx in direct_escapes {
        let Some(param) = params.get(param_idx) else {
            continue;
        };
        if let Some(hop) = find_hop_in_stmt(&param.name, body, summaries, types) {
            hops.insert(param_idx, hop);
        }
    }
    hops
}

fn find_hop_in_stmt(
    param_name: &str,
    stmt: &Statement,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    types: &HashMap<usize, Type>,
) -> Option<EscapeNextHop> {
    match &stmt.node {
        StatementKind::Return(Some(e)) => find_hop_in_return_expr(param_name, e, summaries, types),
        StatementKind::Block(stmts) => {
            for s in stmts {
                if let Some(h) = find_hop_in_stmt(param_name, s, summaries, types) {
                    return Some(h);
                }
            }
            None
        }
        StatementKind::Expression(e) => find_hop_in_stmt_expr(param_name, e, summaries, types),
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    if let Some(h) = find_hop_in_stmt_expr(param_name, init, summaries, types) {
                        return Some(h);
                    }
                }
            }
            None
        }
        StatementKind::If(_, then, else_, _) => {
            if let Some(h) = find_hop_in_stmt(param_name, then, summaries, types) {
                return Some(h);
            }
            if let Some(e) = else_ {
                find_hop_in_stmt(param_name, e, summaries, types)
            } else {
                None
            }
        }
        StatementKind::While(_, body, _) => find_hop_in_stmt(param_name, body, summaries, types),
        StatementKind::For(_, _, body) | StatementKind::GpuFrame(_, _, body) => {
            find_hop_in_stmt(param_name, body, summaries, types)
        }
        StatementKind::Forall { body, .. } => find_hop_in_stmt(param_name, body, summaries, types),
        _ => None,
    }
}

fn find_hop_in_return_expr(
    param_name: &str,
    expr: &Expression,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    types: &HashMap<usize, Type>,
) -> Option<EscapeNextHop> {
    match &expr.node {
        ExpressionKind::Identifier(name, _) if name == param_name => Some(EscapeNextHop::Return),
        ExpressionKind::List(elems) | ExpressionKind::Tuple(elems) | ExpressionKind::Set(elems) => {
            if elems.iter().any(|e| expr_contains_param(e, param_name)) {
                Some(EscapeNextHop::ReturnAggregate)
            } else {
                None
            }
        }
        ExpressionKind::Array(elems, _) => {
            if elems.iter().any(|e| expr_contains_param(e, param_name)) {
                Some(EscapeNextHop::ReturnAggregate)
            } else {
                None
            }
        }
        ExpressionKind::EnumValue(_, vals) => {
            if vals.iter().any(|e| expr_contains_param(e, param_name)) {
                Some(EscapeNextHop::ReturnAggregate)
            } else {
                None
            }
        }
        ExpressionKind::Call(callee, args) => {
            find_hop_in_call_expr(param_name, callee, args, summaries, types)
        }
        ExpressionKind::Lambda(_) => Some(EscapeNextHop::ClosureCapture),
        ExpressionKind::Conditional(then, _, else_, _) => {
            if let Some(h) = find_hop_in_return_expr(param_name, then, summaries, types) {
                return Some(h);
            }
            if let Some(e) = else_ {
                find_hop_in_return_expr(param_name, e, summaries, types)
            } else {
                None
            }
        }
        ExpressionKind::Block(_, final_expr) => {
            find_hop_in_return_expr(param_name, final_expr, summaries, types)
        }
        ExpressionKind::NamedArgument(_, val) => {
            find_hop_in_return_expr(param_name, val, summaries, types)
        }
        _ => None,
    }
}

fn find_hop_in_stmt_expr(
    param_name: &str,
    expr: &Expression,
    summaries: &HashMap<FunctionId, EscapeSummary>,
    types: &HashMap<usize, Type>,
) -> Option<EscapeNextHop> {
    match &expr.node {
        ExpressionKind::Call(callee, args) => {
            find_hop_in_call_expr(param_name, callee, args, summaries, types)
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            // self.field = param → FieldStore
            if let LeftHandSideExpression::Member(member_expr) = lhs.as_ref() {
                if let ExpressionKind::Member(obj, field_expr) = &member_expr.node {
                    if let ExpressionKind::Identifier(obj_name, _) = &obj.node {
                        if obj_name == "self" && expr_contains_param(rhs, param_name) {
                            let field = match &field_expr.node {
                                ExpressionKind::Identifier(f, _) => f.clone(),
                                _ => "?".to_string(),
                            };
                            return Some(EscapeNextHop::FieldStore { field });
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn find_hop_in_call_expr(
    param_name: &str,
    callee: &Expression,
    args: &[Expression],
    summaries: &HashMap<FunctionId, EscapeSummary>,
    types: &HashMap<usize, Type>,
) -> Option<EscapeNextHop> {
    match &callee.node {
        // Free function call: f(param)
        ExpressionKind::Identifier(fn_name, _) => {
            let arg_pos = args
                .iter()
                .position(|a| expr_contains_param(a, param_name))?;
            let summary = summaries.get(fn_name.as_str())?;
            if summary.directly_escapes(arg_pos) {
                Some(EscapeNextHop::Call {
                    callee: fn_name.clone(),
                    param_slot: arg_pos,
                })
            } else {
                None
            }
        }
        // Method call: obj.method(param) — slot 0 = self/receiver
        ExpressionKind::Member(obj, method_expr) => {
            let ExpressionKind::Identifier(method_name, _) = &method_expr.node else {
                return None;
            };
            let receiver_ty = types.get(&obj.id)?;
            let TypeKind::Custom(class_name, _) = &receiver_ty.kind else {
                return None;
            };
            let callee_key = format!("{class_name}_{method_name}");
            let summary = summaries.get(&callee_key)?;

            // Is `param_name` the receiver itself?
            if expr_contains_param(obj, param_name) && summary.directly_escapes(0) {
                return Some(EscapeNextHop::Call {
                    callee: callee_key,
                    param_slot: 0,
                });
            }
            // Is `param_name` one of the regular args?
            let arg_pos = args
                .iter()
                .position(|a| expr_contains_param(a, param_name))?;
            let escape_slot = arg_pos + 1; // +1 because self is slot 0
            if summary.directly_escapes(escape_slot) {
                Some(EscapeNextHop::Call {
                    callee: callee_key,
                    param_slot: escape_slot,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns true if `expr` (or a direct named-arg wrapper) is an identifier
/// equal to `param_name`.
fn expr_contains_param(expr: &Expression, param_name: &str) -> bool {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => name == param_name,
        ExpressionKind::NamedArgument(_, val) => expr_contains_param(val, param_name),
        _ => false,
    }
}
