// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Escape summary data structures for Phase 12 escape-based use-after-move
//! inference on managed types.
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

use std::collections::{BTreeSet, HashMap};

use serde::Deserialize;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::StatementKind;
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

// ── Value-flow rule for return / aggregate escape ─────────────────────────────

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
            // Rule 1: `return p` — the identifier IS or aliases p's heap.
            ExpressionKind::Identifier(name, _) => {
                if let Some(idx) = self.param_index(name) {
                    if aliases_return && self.is_managed_expr(expr) {
                        flow.direct_escapes.insert(idx);
                        flow.return_aliases.insert(idx);
                    }
                }
            }

            // Rule 2: aggregate construction.  Each element flows into the
            // returned aggregate, so the alias context is preserved.
            ExpressionKind::List(elems)
            | ExpressionKind::Tuple(elems)
            | ExpressionKind::Set(elems) => {
                for e in elems {
                    self.classify(e, aliases_return, flow);
                }
            }
            ExpressionKind::Array(elems, _) => {
                for e in elems {
                    self.classify(e, aliases_return, flow);
                }
            }
            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.classify(k, aliases_return, flow);
                    self.classify(v, aliases_return, flow);
                }
            }
            // Enum/struct constructor calls (`Some(p)`, `Pair(p, q)`) reach
            // here when the parser emits an `EnumValue` node; the constructor
            // form `Pair(p, q)` more often surfaces as an `ExpressionKind::Call`
            // and is handled by the call arm below.
            ExpressionKind::EnumValue(_, values) => {
                for v in values {
                    self.classify(v, aliases_return, flow);
                }
            }

            // Rule 3 / Rule 4: projection.  The result expression's TYPE tells
            // us whether the projection alias-preserves (managed) or
            // value-copies (auto-copy).  An auto-copy projection breaks the
            // alias chain — descending into the object stops contributing to
            // the return flow.
            ExpressionKind::Index(obj, idx_expr) => {
                let alias_through = aliases_return && self.is_managed_expr(expr);
                self.classify(obj, alias_through, flow);
                // The index value (e.g. `i` in `p[i]`) does not flow into the
                // returned element; its own subexpressions can still carry
                // consuming side-effects via callee calls, but we only walk it with
                // alias_return=false to avoid double-counting.
                self.classify(idx_expr, false, flow);
            }
            ExpressionKind::Member(obj, _) => {
                let alias_through = aliases_return && self.is_managed_expr(expr);
                self.classify(obj, alias_through, flow);
            }

            // Rules 5–7: call boundaries consult the callee's escape summary.
            ExpressionKind::Call(callee, args) => {
                self.classify_call(callee, args, aliases_return, flow);
            }

            // Wrappers that pass through their inner value untouched.
            ExpressionKind::NamedArgument(_, val) => {
                self.classify(val, aliases_return, flow);
            }
            ExpressionKind::Conditional(then_expr, cond_expr, else_expr, _) => {
                // ExpressionKind::Conditional carries fields in the order
                // (then, cond, else?) — see src/parser/expressions/control_flow.rs
                // and src/mir/lowering/expression/conditional_expr.rs.  The
                // condition does not flow into the value; branches do.
                self.classify(cond_expr, false, flow);
                self.classify(then_expr, aliases_return, flow);
                if let Some(e) = else_expr {
                    self.classify(e, aliases_return, flow);
                }
            }
            ExpressionKind::Block(_, final_expr) => {
                // Statement effects in a return-position block are handled
                // elsewhere; only the trailing expression contributes to
                // the return value's heap aliasing.
                self.classify(final_expr, aliases_return, flow);
            }
            ExpressionKind::Match(scrutinee, branches) => {
                // The scrutinee itself does not flow into the result.
                self.classify(scrutinee, false, flow);
                for branch in branches {
                    if let Some(guard) = &branch.guard {
                        self.classify(guard, false, flow);
                    }
                    self.classify_stmt_for_value(&branch.body, aliases_return, flow);
                }
            }

            // Operators producing primitive results: never alias-creating.
            // Recurse with alias_return=false so any nested calls still get
            // their consuming-arg semantics — but no managed identifier here
            // can flow into the return value of `return a + b`.
            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.classify(l, false, flow);
                self.classify(r, false, flow);
            }
            ExpressionKind::Unary(_, e) | ExpressionKind::Guard(_, e) => {
                self.classify(e, false, flow);
            }
            ExpressionKind::Range(start, end, _) => {
                self.classify(start, false, flow);
                if let Some(e) = end {
                    self.classify(e, false, flow);
                }
            }
            ExpressionKind::FormattedString(parts) => {
                for p in parts {
                    self.classify(p, false, flow);
                }
            }
            ExpressionKind::Assignment(_, _, rhs) => {
                // An assignment expression's result is the rhs value.
                self.classify(rhs, aliases_return, flow);
            }

            // Lambda capture — handled separately.  Closures returned
            // verbatim are handled at a higher level.
            ExpressionKind::Lambda(_) => {}

            // Leaves that produce no value-flow into a managed return:
            ExpressionKind::Literal(_)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Super => {}
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

// ── TOML deserialization helpers ──────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_summary_is_empty() {
        let s = EscapeSummary::default();
        assert!(s.is_empty());
        assert!(s.direct_escapes.is_empty());
        assert!(s.conditional_escapes.is_empty());
        assert!(s.return_aliases.is_empty());
    }

    #[test]
    fn direct_escape_membership() {
        let mut s = EscapeSummary::default();
        s.direct_escapes.insert(0);
        s.direct_escapes.insert(2);
        assert!(s.directly_escapes(0));
        assert!(!s.directly_escapes(1));
        assert!(s.directly_escapes(2));
        assert!(!s.is_empty());
    }

    #[test]
    fn conditional_escape_roundtrip() {
        let ce = ConditionalEscape {
            param: 0,
            via_fn_param: 1,
            callee_param: 0,
        };
        let mut s = EscapeSummary::default();
        s.conditional_escapes.push(ce.clone());
        assert_eq!(s.conditional_escapes.len(), 1);
        assert_eq!(s.conditional_escapes[0], ce);
        assert!(!s.is_empty());
    }

    #[test]
    fn return_aliases_membership() {
        let mut s = EscapeSummary::default();
        s.return_aliases.insert(1);
        assert!(s.return_aliases.contains(&1));
        assert!(!s.return_aliases.contains(&0));
    }

    #[test]
    fn equality_and_clone() {
        let mut a = EscapeSummary::default();
        a.direct_escapes.insert(0);
        let b = a.clone();
        assert_eq!(a, b);
        a.direct_escapes.insert(1);
        assert_ne!(a, b);
    }

    // ── FFI summary loading tests ────────────────────────────────────────────────

    #[test]
    fn load_ffi_summaries_parses_without_panic() {
        let summaries = load_ffi_summaries();
        assert!(
            !summaries.is_empty(),
            "escape_summaries.toml should have at least one entry"
        );
    }

    #[test]
    fn list_push_escapes_element() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_list_push")
            .expect("miri_rt_list_push must have a summary");
        // param 1 (val) escapes into the list
        assert!(s.directly_escapes(1), "val (param 1) must escape");
        // param 0 (the raw list pointer) is unmanaged — not listed
        assert!(!s.directly_escapes(0));
        assert!(s.conditional_escapes.is_empty());
        assert!(s.return_aliases.is_empty());
    }

    #[test]
    fn map_set_escapes_key_and_value() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_map_set")
            .expect("miri_rt_map_set must have a summary");
        assert!(s.directly_escapes(1), "key (param 1) must escape");
        assert!(s.directly_escapes(2), "value (param 2) must escape");
        assert!(!s.directly_escapes(0));
    }

    #[test]
    fn set_add_escapes_element() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_set_add")
            .expect("miri_rt_set_add must have a summary");
        assert!(s.directly_escapes(1), "elem (param 1) must escape");
        assert!(!s.directly_escapes(0));
    }

    #[test]
    fn io_sinks_have_no_escapes() {
        let summaries = load_ffi_summaries();
        for name in &[
            "miri_rt_print",
            "miri_rt_println",
            "miri_rt_eprint",
            "miri_rt_eprintln",
        ] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
            assert!(
                s.is_empty(),
                "{name} is an IO sink — no parameters should escape"
            );
        }
    }

    #[test]
    fn list_insert_and_set_escape_element() {
        let summaries = load_ffi_summaries();
        for name in &["miri_rt_list_insert", "miri_rt_list_set"] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have a summary"));
            assert!(s.directly_escapes(2), "{name}: val (param 2) must escape");
        }
    }

    #[test]
    fn array_set_val_escapes_element() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_array_set_val")
            .expect("miri_rt_array_set_val must have a summary");
        assert!(s.directly_escapes(2), "val (param 2) must escape");
        assert!(!s.directly_escapes(0));
        assert!(!s.directly_escapes(1));
    }

    #[test]
    fn map_read_only_accessors_have_no_escapes() {
        let summaries = load_ffi_summaries();
        for name in &[
            "miri_rt_map_get",
            "miri_rt_map_contains_key",
            "miri_rt_map_remove",
        ] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
            assert!(
                s.is_empty(),
                "{name} is a read-only accessor — no parameters should escape"
            );
        }
    }

    #[test]
    fn set_read_only_accessors_have_no_escapes() {
        let summaries = load_ffi_summaries();
        for name in &["miri_rt_set_contains", "miri_rt_set_remove"] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
            assert!(
                s.is_empty(),
                "{name} is a read-only accessor — no parameters should escape"
            );
        }
    }

    // ── Value-flow rule: analyze_return_value ───────────────────────────────────
    //
    // These tests cover each of the 7 enumerated rule cases by
    // hand-building small return expressions and the supporting types map.
    // They exercise the analyzer in isolation; integration with the
    // call-graph fixpoint is deferred.

    use crate::ast::expression::{Expression, ExpressionKind};
    use crate::ast::factory::{
        call_with_span, conditional_with_span, expr_with_span, identifier_with_span,
        index_with_span, list_with_span, make_type, member_with_span, tuple_with_span,
    };
    use crate::ast::statement::IfStatementType;
    use crate::ast::types::{Type, TypeKind};
    use crate::ast::Parameter;
    use crate::error::syntax::Span;
    use crate::type_checker::context::TypeDefinition;

    /// Builds a `Parameter` with the given name and a placeholder type
    /// expression — the analyzer never reads `Parameter::typ`, only `name`.
    fn param(name: &str) -> Parameter {
        Parameter {
            name: name.to_string(),
            typ: Box::new(expr_with_span(
                ExpressionKind::Type(Box::new(make_type(TypeKind::Int)), false),
                Span::new(0, 0),
            )),
            guard: None,
            default_value: None,
            is_out: false,
        }
    }

    /// Managed type used as a stand-in for "any heap type": `Custom("List", _)`
    /// with no `type_definitions` entry → `is_auto_copy` returns `false`, so
    /// the analyzer treats it as managed (alias-creating).
    fn managed_type() -> Type {
        make_type(TypeKind::Custom("List".to_string(), None))
    }

    fn primitive_type() -> Type {
        make_type(TypeKind::Int)
    }

    /// Creates an `Identifier` expression with the given name and registers a
    /// type for it in `types`.
    fn ident(name: &str, ty: Type, types: &mut HashMap<usize, Type>) -> Expression {
        let e = identifier_with_span(name, Span::new(0, 0));
        types.insert(e.id, ty);
        e
    }

    /// Wraps `expr` in a fresh outer expression `Index(expr, _)` registering
    /// `result_ty` (the element type) at the new node's id.
    fn index(
        obj: Expression,
        idx: Expression,
        result_ty: Type,
        types: &mut HashMap<usize, Type>,
    ) -> Expression {
        let e = index_with_span(obj, idx, Span::new(0, 0));
        types.insert(e.id, result_ty);
        e
    }

    fn member(
        obj: Expression,
        field_name: &str,
        result_ty: Type,
        types: &mut HashMap<usize, Type>,
    ) -> Expression {
        let field = identifier_with_span(field_name, Span::new(0, 0));
        let e = member_with_span(obj, field, Span::new(0, 0));
        types.insert(e.id, result_ty);
        e
    }

    fn list_lit(elems: Vec<Expression>, ty: Type, types: &mut HashMap<usize, Type>) -> Expression {
        let e = list_with_span(elems, Span::new(0, 0));
        types.insert(e.id, ty);
        e
    }

    fn tuple_lit(elems: Vec<Expression>, ty: Type, types: &mut HashMap<usize, Type>) -> Expression {
        let e = tuple_with_span(elems, Span::new(0, 0));
        types.insert(e.id, ty);
        e
    }

    /// Builds a call `name(args)` where `name` is a free function identifier.
    /// The callee identifier and the call expression are typed as managed by
    /// default to keep the alias chain alive; a primitive return type can be
    /// supplied via `return_ty` to test rule 6.
    fn call(
        name: &str,
        args: Vec<Expression>,
        return_ty: Type,
        types: &mut HashMap<usize, Type>,
    ) -> Expression {
        let callee = identifier_with_span(name, Span::new(0, 0));
        // Callee identifier itself doesn't matter for type-tracking; it is not
        // an Identifier *of a parameter* in the tests below, so the analyzer
        // ignores its type.  Register a placeholder for completeness.
        types.insert(callee.id, primitive_type());
        let e = call_with_span(callee, args, Span::new(0, 0));
        types.insert(e.id, return_ty);
        e
    }

    fn empty_summaries() -> HashMap<FunctionId, EscapeSummary> {
        HashMap::new()
    }

    // ── Rule 1: `return p` ────────────────────────────────────────────────────

    #[test]
    fn rule1_return_managed_param_escapes_and_aliases() {
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("items")];
        let ret = ident("items", managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.contains(&0));
        assert!(flow.return_aliases.contains(&0));
    }

    #[test]
    fn rule1_return_primitive_param_does_not_escape() {
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("n")];
        let ret = ident("n", primitive_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.is_empty());
        assert!(flow.return_aliases.is_empty());
    }

    // ── Rule 2: aggregate construction ────────────────────────────────────────

    #[test]
    fn rule2_return_list_of_managed_params_escapes_each() {
        // `return [p, q]` where p, q are managed parameters.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p"), param("q")];
        let p = ident("p", managed_type(), &mut types);
        let q = ident("q", managed_type(), &mut types);
        let ret = list_lit(vec![p, q], managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.contains(&0));
        assert!(flow.direct_escapes.contains(&1));
        assert!(flow.return_aliases.contains(&0));
        assert!(flow.return_aliases.contains(&1));
    }

    #[test]
    fn rule2_return_tuple_of_managed_params_escapes_each() {
        // `return Pair(p, q)` represented as a tuple constructor.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p"), param("q")];
        let p = ident("p", managed_type(), &mut types);
        let q = ident("q", managed_type(), &mut types);
        let ret = tuple_lit(vec![p, q], managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert_eq!(
            flow.direct_escapes,
            BTreeSet::from([0_usize, 1_usize]),
            "both params must be in direct_escapes"
        );
        assert_eq!(flow.return_aliases, BTreeSet::from([0_usize, 1_usize]));
    }

    // ── Rule 3: `return p[i]` — managed vs primitive element ──────────────────

    #[test]
    fn rule3_index_managed_element_escapes() {
        // `return p[i]` where p has type List<List<int>>; element type managed.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p"), param("i")];
        let p = ident("p", managed_type(), &mut types);
        let i = ident("i", primitive_type(), &mut types);
        let ret = index(p, i, managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.contains(&0));
        assert!(flow.return_aliases.contains(&0));
        // The integer index parameter does not flow into the return.
        assert!(!flow.direct_escapes.contains(&1));
    }

    #[test]
    fn rule3_index_primitive_element_does_not_escape() {
        // `return p[i]` where p has type List<int>; element type is auto-copy.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p"), param("i")];
        let p = ident("p", managed_type(), &mut types);
        let i = ident("i", primitive_type(), &mut types);
        let ret = index(p, i, primitive_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.is_empty());
        assert!(flow.return_aliases.is_empty());
    }

    // ── Rule 4: `return p.field` — managed vs primitive field ────────────────

    #[test]
    fn rule4_member_managed_field_escapes() {
        // `return p.cache` where cache: List<int>.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        let ret = member(p, "cache", managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.contains(&0));
        assert!(flow.return_aliases.contains(&0));
    }

    #[test]
    fn rule4_member_primitive_field_does_not_escape() {
        // `return p.count` where count: int.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        let ret = member(p, "count", primitive_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.is_empty());
        assert!(flow.return_aliases.is_empty());
    }

    // ── Rule 5: `return f(p)` where f consumes param 0 ──────────────────────

    #[test]
    fn rule5_call_consumes_param_via_sink_chain() {
        // `return store(p)` where `store`'s param 0 is in direct_escapes.
        // Expected: p in direct_escapes (consumed via f's sink), but the
        // call's return value is independent of p's heap (f.return_aliases
        // is empty), so p is NOT in our return_aliases.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
        summaries.insert(
            "store".to_string(),
            EscapeSummary {
                direct_escapes: BTreeSet::from([0_usize]),
                ..EscapeSummary::default()
            },
        );
        let ret = call("store", vec![p], managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

        assert!(
            flow.direct_escapes.contains(&0),
            "p must be in direct_escapes (consumed by f's sink chain)"
        );
        assert!(
            !flow.return_aliases.contains(&0),
            "p must NOT be in return_aliases (f's return is independent of p's heap)"
        );
    }

    // ── Rule 6: `return f(p)` where f neither escapes nor return-aliases 0 ─

    #[test]
    fn rule6_call_neither_consumes_nor_aliases() {
        // `return length_of(p)` where length_of has empty escape summary.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
        summaries.insert("length_of".to_string(), EscapeSummary::default());
        let ret = call("length_of", vec![p], primitive_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

        assert!(flow.direct_escapes.is_empty());
        assert!(flow.return_aliases.is_empty());
    }

    // ── Rule 7: `return f(p)` where f.return_aliases ∋ 0 ────────────────────

    #[test]
    fn rule7_call_return_aliases_param_propagates_alias() {
        // `return identity(p)` where `identity`'s return aliases param 0.
        // Expected: p in BOTH direct_escapes and return_aliases (the call's
        // return value aliases p's heap, and our return is that call's value).
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
        summaries.insert(
            "identity".to_string(),
            EscapeSummary {
                return_aliases: BTreeSet::from([0_usize]),
                ..EscapeSummary::default()
            },
        );
        let ret = call("identity", vec![p], managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

        assert!(flow.direct_escapes.contains(&0));
        assert!(flow.return_aliases.contains(&0));
    }

    #[test]
    fn rule7_call_return_aliases_only_when_outer_return_alias_holds() {
        // Even if a callee's return aliases its arg, that does not propagate
        // to *our* return when the call's value does not flow into our return
        // (e.g., the call appears under an auto-copy projection).
        // `return identity(p).length` — `.length` is primitive, so the call's
        // managed return is dropped at the projection step.  No aliasing
        // contribution from the call.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
        summaries.insert(
            "identity".to_string(),
            EscapeSummary {
                return_aliases: BTreeSet::from([0_usize]),
                ..EscapeSummary::default()
            },
        );
        let inner_call = call("identity", vec![p], managed_type(), &mut types);
        let ret = member(inner_call, "length", primitive_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

        assert!(
            flow.direct_escapes.is_empty(),
            "primitive projection breaks the alias chain — p must not escape"
        );
        assert!(flow.return_aliases.is_empty());
    }

    // ── Bonus coverage: behaviour at unresolved callees ────────────────────────
    //
    // The conservative policy ("every managed param escapes") for unresolved
    // callees is the escape analysis pass's responsibility, not this value-flow
    // analyzer's.  In isolation, the analyzer makes no escape claim for an
    // unresolved callee — it simply does not propagate the alias context.
    // This guard pins that behaviour so the pass has a known baseline.

    #[test]
    fn unresolved_callee_makes_no_escape_claim() {
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let p = ident("p", managed_type(), &mut types);
        // Empty summaries map — the analyzer does NOT find `unknown_fn`.
        let summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
        let ret = call("unknown_fn", vec![p], managed_type(), &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

        assert!(
            flow.direct_escapes.is_empty(),
            "the value-flow analyzer alone does not enforce the conservative default for unresolved callees"
        );
        assert!(flow.return_aliases.is_empty());
    }

    // ── Identifier referring to a non-parameter is ignored ────────────────────

    #[test]
    fn return_local_variable_is_ignored() {
        // The analyzer only classifies parameter identifiers; a return of a
        // local variable contributes nothing to the parameter-indexed flow.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let local = ident("local", managed_type(), &mut types);

        let flow =
            analyze_return_value(&local, &params, &types, &HashMap::new(), &empty_summaries());

        assert!(flow.direct_escapes.is_empty());
        assert!(flow.return_aliases.is_empty());
    }

    // ── Auto-copy struct test: managed-looking by name but auto-copy ─────────
    //
    // When a parameter's type is a small POD struct registered in
    // `type_definitions` whose fields are all primitives, `is_auto_copy`
    // returns true and the alias chain breaks at the param identifier itself.
    // This pins down the "primitive types do not escape" half of rule 1.

    #[test]
    fn auto_copy_struct_param_does_not_escape() {
        use crate::type_checker::context::StructDefinition;

        let mut type_defs: HashMap<String, TypeDefinition> = HashMap::new();
        type_defs.insert(
            "Point".to_string(),
            TypeDefinition::Struct(StructDefinition {
                fields: vec![
                    (
                        "x".to_string(),
                        make_type(TypeKind::Int),
                        crate::ast::common::MemberVisibility::Public,
                    ),
                    (
                        "y".to_string(),
                        make_type(TypeKind::Int),
                        crate::ast::common::MemberVisibility::Public,
                    ),
                ],
                generics: None,
                module: "test".to_string(),
                has_drop: false,
            }),
        );
        let point_ty = make_type(TypeKind::Custom("Point".to_string(), None));
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p")];
        let ret = ident("p", point_ty, &mut types);

        let flow = analyze_return_value(&ret, &params, &types, &type_defs, &empty_summaries());

        assert!(
            flow.direct_escapes.is_empty(),
            "auto-copy struct param does not flow into return alias"
        );
        assert!(flow.return_aliases.is_empty());
    }

    // ── Conditional in return position ────────────────────────────────────────
    //
    // Regression guard for the field-order bug in `ExpressionKind::Conditional`:
    // the variant carries `(then, cond, else?)`, not `(cond, then, else?)`.
    // A both-branches-managed-param test catches a swap because it requires
    // both arms to be walked with `aliases_return=true`; a swap would silently
    // walk the then-branch with `false` and miss the escape.

    #[test]
    fn conditional_branches_propagate_alias_to_both() {
        // `return (cond ? p : q)` where p, q are managed params; cond is some
        // primitive expression that does not reference p or q.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("p"), param("q"), param("cond_local")];
        let p = ident("p", managed_type(), &mut types);
        let q = ident("q", managed_type(), &mut types);
        let cond = ident("cond_local", primitive_type(), &mut types);
        let if_expr = conditional_with_span(p, cond, Some(q), IfStatementType::If, Span::new(0, 0));
        types.insert(if_expr.id, managed_type());

        let flow = analyze_return_value(
            &if_expr,
            &params,
            &types,
            &HashMap::new(),
            &empty_summaries(),
        );

        assert!(
            flow.direct_escapes.contains(&0),
            "then-branch param p must be in direct_escapes"
        );
        assert!(
            flow.direct_escapes.contains(&1),
            "else-branch param q must be in direct_escapes"
        );
        assert!(
            !flow.direct_escapes.contains(&2),
            "the condition expression's identifier must not flow into the value"
        );
        assert!(flow.return_aliases.contains(&0));
        assert!(flow.return_aliases.contains(&1));
    }

    #[test]
    fn conditional_managed_param_in_condition_does_not_escape() {
        // Catches the inverse of the field-order bug: a managed-typed param
        // referenced in the *condition* must NOT escape via the return value.
        // The current canonical layout is `Conditional(then, cond, else?)`.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("flag"), param("a"), param("b")];
        // `flag` is managed (e.g. an Option<bool>) — alias-creating IF wrongly
        // walked with aliases_return=true.
        let flag = ident("flag", managed_type(), &mut types);
        let a = ident("a", primitive_type(), &mut types);
        let b = ident("b", primitive_type(), &mut types);
        let if_expr = conditional_with_span(a, flag, Some(b), IfStatementType::If, Span::new(0, 0));
        // Result type is primitive → the conditional's value cannot alias
        // anyone's heap, but the analyzer should reach this conclusion
        // regardless of result type because the *branches* are primitive.
        types.insert(if_expr.id, primitive_type());

        let flow = analyze_return_value(
            &if_expr,
            &params,
            &types,
            &HashMap::new(),
            &empty_summaries(),
        );

        assert!(
            !flow.direct_escapes.contains(&0),
            "param `flag` is only in the condition — it must not escape"
        );
        assert!(flow.direct_escapes.is_empty());
        assert!(flow.return_aliases.is_empty());
    }

    // ── Method dispatch summary lookup (`ClassName_method` key) ───────────────
    //
    // Pins the resolve_callee_summary path for `obj.method(p)`: the receiver
    // becomes summary slot 0, and `args` shift by 1.

    #[test]
    fn method_call_consumes_receiver_via_class_method_key() {
        // `return cache.store(p)` where Cache_store has direct_escapes = {0, 1}.
        // Receiver `cache` is summary slot 0, arg `p` is slot 1; both must be
        // marked direct-escape via rule 5.
        let mut types: HashMap<usize, Type> = HashMap::new();
        let params = vec![param("cache"), param("p")];
        let cache = ident(
            "cache",
            make_type(TypeKind::Custom("Cache".to_string(), None)),
            &mut types,
        );
        let p = ident("p", managed_type(), &mut types);
        let store_method = identifier_with_span("store", Span::new(0, 0));
        types.insert(store_method.id, primitive_type());
        let callee = member_with_span(cache, store_method, Span::new(0, 0));
        types.insert(callee.id, primitive_type());
        let ret = call_with_span(callee, vec![p], Span::new(0, 0));
        types.insert(ret.id, managed_type());

        let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
        summaries.insert(
            "Cache_store".to_string(),
            EscapeSummary {
                direct_escapes: BTreeSet::from([0_usize, 1_usize]),
                ..EscapeSummary::default()
            },
        );

        let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

        assert!(
            flow.direct_escapes.contains(&0),
            "receiver `cache` must be marked direct-escape via Cache_store slot 0"
        );
        assert!(
            flow.direct_escapes.contains(&1),
            "arg `p` must be marked direct-escape via Cache_store slot 1"
        );
        assert!(flow.return_aliases.is_empty());
    }
}
