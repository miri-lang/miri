// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Use-after-move analysis for the Miri type checker.
//!
//! Tracks which variables have been "consumed" and emits a compile-time error
//! if a consumed variable is subsequently accessed. A variable is consumed by:
//! - Passing it as an argument to a function call (§7.1)
//! - Assigning it to another variable when the type is a resource (§7.5)
//!
//! Auto-copy types (size ≤ 128 bytes, all-primitive fields) are always copied
//! and are never flagged. Resource types (`fn drop(self)` defined) are always
//! tracked; other managed types are only tracked at top-level scope.

use crate::ast::expression::{ExpressionKind, LeftHandSideExpression};
use crate::ast::pattern::Pattern;
use crate::ast::statement::StatementKind;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::diagnostic::{Diagnostic, Severity};
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};
use std::collections::{HashMap, HashSet};

use super::context::TypeDefinition;
use super::escape_analysis::{EscapeSummary, FunctionId};
use super::utils::{is_auto_copy, is_resource};

#[derive(Clone)]
struct ConsumedInfo {
    by_fn: String,
    #[allow(dead_code)]
    at_span: Span,
}

pub struct UseAfterMoveChecker<'a> {
    types: &'a HashMap<usize, Type>,
    type_definitions: &'a HashMap<String, TypeDefinition>,
    /// Escape summaries keyed by `FunctionId` (`"fn_name"` or `"ClassName_method"`).
    /// Pre-populated with FFI summaries (§12.0.2); user-method summaries added by §12.1.
    escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    errors: Vec<TypeError>,
    warnings: Vec<Diagnostic>,
    /// True when analysis is inside a function body (vs. top-level script code).
    /// Resource types are always tracked. Managed types are only tracked at top level.
    in_function_body: bool,
    /// Names currently bound as parameters or `let`/`var` locals.  Used by the
    /// §12.0.6 dynamic-fn detector to distinguish a literal callee identifier
    /// (a free function declared at module scope — never re-bound, so absent
    /// here) from a dynamic fn-value bound to a name (let-bound, parameter,
    /// re-assigned).  A `Call` whose callee identifier is in this set is a
    /// dynamic call and consumes every managed-typed argument.
    ///
    /// The set is snapshotted on entry to a function body and restored on exit
    /// so an inner function does not see its caller's bindings.
    fn_bindings: HashSet<String>,
}

impl<'a> UseAfterMoveChecker<'a> {
    pub fn new(
        types: &'a HashMap<usize, Type>,
        type_definitions: &'a HashMap<String, TypeDefinition>,
        escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    ) -> Self {
        Self {
            types,
            type_definitions,
            escape_summaries,
            errors: vec![],
            warnings: vec![],
            in_function_body: false,
            fn_bindings: HashSet::new(),
        }
    }

    /// Runs the analysis on a complete program and returns detected errors and warnings.
    pub fn check_program(mut self, program: &Program) -> (Vec<TypeError>, Vec<Diagnostic>) {
        let mut consumed: HashMap<String, ConsumedInfo> = HashMap::new();
        self.check_block(&program.body, &mut consumed);
        (self.errors, self.warnings)
    }

    fn check_stmt(&mut self, stmt: &Statement, consumed: &mut HashMap<String, ConsumedInfo>) {
        match &stmt.node {
            // Analyze function bodies with resource-only tracking: managed types are never
            // flagged inside function bodies (read-only recursive algorithms pass the same
            // managed value to multiple calls without consuming it). Only resource types
            // (those defining `fn drop(self)`) are tracked.
            StatementKind::FunctionDeclaration(decl) => {
                if let Some(body) = &decl.body {
                    let prev_in_fn = self.in_function_body;
                    let prev_bindings = std::mem::take(&mut self.fn_bindings);
                    self.in_function_body = true;
                    // Parameters are bindings inside the body — by §12.0.6, a
                    // call whose callee identifier matches a parameter name is
                    // dynamic.  We add every parameter name; whether the param
                    // is fn-typed is checked at the call site (an Identifier
                    // callee that isn't fn-typed wouldn't type-check anyway).
                    for p in &decl.params {
                        self.fn_bindings.insert(p.name.clone());
                    }
                    let mut fn_consumed: HashMap<String, ConsumedInfo> = HashMap::new();
                    self.check_stmt(body, &mut fn_consumed);
                    self.in_function_body = prev_in_fn;
                    self.fn_bindings = prev_bindings;
                }
            }

            StatementKind::Block(stmts) => {
                self.check_block(stmts, consumed);
            }

            StatementKind::Expression(expr) => {
                self.check_expr(expr, consumed);
            }

            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    if let Some(init) = &decl.initializer {
                        self.check_expr(init, consumed);
                        // §7.5: assignment of a resource type is a move — mark the
                        // source identifier consumed regardless of scope.
                        if let ExpressionKind::Identifier(src, _) = &init.node {
                            if let Some(ty) = self.types.get(&init.id) {
                                if is_resource(&ty.kind, self.type_definitions) {
                                    consumed.insert(
                                        src.clone(),
                                        ConsumedInfo {
                                            by_fn: format!("assignment to '{}'", decl.name),
                                            at_span: init.span,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    // §12.0.6: every let/var local is a binding — its callee form
                    // (`name(...)`) is a dynamic-fn call, never a literal.
                    self.fn_bindings.insert(decl.name.clone());
                }
            }

            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.check_expr(e, consumed);
                }
            }

            StatementKind::If(cond, then, else_, _) => {
                self.check_expr(cond, consumed);

                // Each branch gets a snapshot of the pre-branch consumed state so
                // that consuming in the then-branch doesn't poison the else-branch
                // or vice-versa.  After the if: only variables consumed in BOTH
                // branches are "definitely consumed".
                let mut then_consumed = consumed.clone();
                self.check_stmt(then, &mut then_consumed);

                let mut else_consumed = consumed.clone();
                if let Some(e) = else_ {
                    self.check_stmt(e, &mut else_consumed);
                }

                *consumed = then_consumed
                    .into_iter()
                    .filter(|(k, _)| else_consumed.contains_key(k))
                    .collect();
            }

            StatementKind::While(cond, body, _) => {
                self.check_expr(cond, consumed);
                self.check_stmt(body, consumed);
            }

            StatementKind::For(decls, iter, body) => {
                self.check_expr(iter, consumed);
                // §12.0.6: the loop pattern variable is a binding inside the
                // body — `for f in fns: f(items)` must classify `f` as a
                // dynamic-fn callee.  Snapshot/restore so the binding does not
                // leak past the loop and accidentally over-consume calls to a
                // shadowed top-level fn after the loop exits.
                let prev_bindings = self.fn_bindings.clone();
                for d in decls {
                    self.fn_bindings.insert(d.name.clone());
                }
                self.check_stmt(body, consumed);
                self.fn_bindings = prev_bindings;
            }

            // Declarations, imports, type aliases — no variable uses.
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Use(_, _)
            | StatementKind::Type(_, _)
            | StatementKind::Enum(_, _, _, _)
            | StatementKind::Struct(_, _, _, _, _)
            | StatementKind::Class(_)
            | StatementKind::Trait(_, _, _, _, _)
            | StatementKind::RuntimeFunctionDeclaration(_, _, _, _) => {}
        }
    }

    /// Processes a block's statement list with scope-exit warning for unconsumed resource vars.
    ///
    /// Variables declared at THIS block level (direct `StatementKind::Variable` stmts) are
    /// tracked. If a resource-typed variable is still live at the end of the block, a W0004
    /// warning is emitted — the drop hook still runs via RC.
    fn check_block(&mut self, stmts: &[Statement], consumed: &mut HashMap<String, ConsumedInfo>) {
        // Collect resource vars declared at this scope level (not in nested blocks).
        let mut scope_resources: Vec<(String, String, Span)> = Vec::new(); // (var_name, type_name, decl_span)

        for s in stmts {
            if let StatementKind::Variable(decls, _) = &s.node {
                for decl in decls {
                    if let Some(init) = &decl.initializer {
                        if let Some(ty) = self.types.get(&init.id) {
                            if is_resource(&ty.kind, self.type_definitions) {
                                if let Some(type_name) = Self::resource_type_name(&ty.kind) {
                                    scope_resources.push((
                                        decl.name.clone(),
                                        type_name.to_string(),
                                        s.span,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            self.check_stmt(s, consumed);
        }

        // At scope exit, warn for resource vars not explicitly consumed.
        for (var_name, type_name, span) in &scope_resources {
            if !consumed.contains_key(var_name.as_str()) {
                self.warnings.push(Diagnostic {
                    severity: Severity::Warning,
                    code: Some("W0004"),
                    title: "resource not consumed at scope exit".to_string(),
                    message: format!(
                        "resource '{}' of type '{}' was not consumed before scope exit",
                        var_name, type_name
                    ),
                    span: Some(*span),
                    help: Some(
                        "pass the resource to a consuming function or call its drop method"
                            .to_string(),
                    ),
                    notes: Vec::new(),
                    source_override: None,
                });
            }
        }
    }

    /// Extracts a display name for the resource type used in the W0004 warning.
    ///
    /// Returns the nominal name for `Custom` types and the parameter name for
    /// `Generic` types (so a resource-bounded generic local appears in the
    /// warning as `type 'T'`, per §12.0.4).
    fn resource_type_name(kind: &TypeKind) -> Option<&str> {
        match kind {
            TypeKind::Custom(name, _) => Some(name.as_str()),
            TypeKind::Generic(name, _, _) => Some(name.as_str()),
            _ => None,
        }
    }

    fn check_expr(&mut self, expr: &Expression, consumed: &mut HashMap<String, ConsumedInfo>) {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => {
                if let Some(info) = consumed.get(name.as_str()) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Custom {
                            message: format!(
                                "'{}' was consumed by '{}' and cannot be used again; call .clone() to make a copy",
                                name, info.by_fn
                            ),
                            help: None,
                        },
                        span: expr.span,
                        source_override: None,
                    });
                }
            }

            ExpressionKind::Call(callee, args) => {
                let fn_name = self.extract_callee_name(callee);

                // §12.0.3: extract the method escape summary (owned) before any
                // mutable borrow of self so the borrow checker stays happy.
                let method_summary: Option<EscapeSummary> = self.extract_method_summary(callee);

                // §12.0.6: classify the callee.  A literal callee is a free-fn
                // identifier (declared at module scope, not in `fn_bindings`) or
                // a method member access.  Anything else — let-bound, branch,
                // returned-from-fn, parameter — is a dynamic fn-value, and the
                // dynamic-fn fallback below treats every managed-typed argument
                // as escaping.
                let is_dynamic_fn = method_summary.is_none() && self.is_dynamic_fn_callee(callee);

                // Check callee for consumed uses (handles method receiver via Member).
                self.check_expr(callee, consumed);

                // Check args — error if any consumed variable is used.
                for arg in args {
                    self.check_expr(arg, consumed);
                }

                // Resolve the function's parameter list so we can skip consuming
                // variables passed to `out` params — the callee writes to those,
                // so the caller's variable remains live after the call.
                let params: &[Parameter] = self
                    .types
                    .get(&callee.id)
                    .and_then(|ty| {
                        if let TypeKind::Function(fd) = &ty.kind {
                            Some(fd.params.as_slice())
                        } else {
                            None
                        }
                    })
                    .unwrap_or(&[]);

                if let Some(ref summary) = method_summary {
                    // §12.0.3 static / virtual path: apply escape summary.
                    // param 0 = self (receiver); params 1..N = explicit args.
                    if summary.directly_escapes(0) {
                        if let ExpressionKind::Member(obj_expr, _) = &callee.node {
                            self.maybe_consume_arg(obj_expr, &fn_name, consumed);
                        }
                    }
                    // Track both the out-param index (into `params`, self-free)
                    // and the escape-summary index (1-based, shifted for self at 0).
                    let mut pos_idx = 0usize;
                    for arg in args {
                        let (is_out, escape_idx) = match &arg.node {
                            ExpressionKind::NamedArgument(name, _) => {
                                let out = params
                                    .iter()
                                    .find(|p| &p.name == name)
                                    .is_some_and(|p| p.is_out);
                                // Named arg: position in params + 1 for self offset.
                                let eidx = params
                                    .iter()
                                    .position(|p| &p.name == name)
                                    .map(|i| i + 1)
                                    .unwrap_or(usize::MAX);
                                (out, eidx)
                            }
                            _ => {
                                let out = params.get(pos_idx).is_some_and(|p| p.is_out);
                                let eidx = pos_idx + 1; // +1 because self is param 0
                                pos_idx += 1;
                                (out, eidx)
                            }
                        };
                        if !is_out && summary.directly_escapes(escape_idx) {
                            self.maybe_consume_arg(arg, &fn_name, consumed);
                        }
                    }
                } else if is_dynamic_fn {
                    // §12.0.6 dynamic fn-valued callee: every managed-typed arg
                    // is conservatively treated as escaping ("via dynamic fn
                    // parameter `f`"), regardless of whether we are at top
                    // level or inside a function body.  Resource args are
                    // already always-consume per §7.4 so the predicate below
                    // accepts both managed and resource — only auto-copy types
                    // are exempt.
                    let sink = format!("dynamic fn '{fn_name}'");
                    let mut pos_idx = 0usize;
                    for arg in args {
                        let is_out = match &arg.node {
                            ExpressionKind::NamedArgument(name, _) => params
                                .iter()
                                .find(|p| &p.name == name)
                                .is_some_and(|p| p.is_out),
                            _ => {
                                let out = params.get(pos_idx).is_some_and(|p| p.is_out);
                                pos_idx += 1;
                                out
                            }
                        };
                        if !is_out {
                            self.consume_arg_dynamic(arg, &sink, consumed);
                        }
                    }
                } else {
                    // Free function call, or method call with no summary available:
                    // fall back to the existing should_consume_expr logic.
                    let mut pos_idx = 0usize;
                    for arg in args {
                        let is_out = match &arg.node {
                            ExpressionKind::NamedArgument(name, _) => params
                                .iter()
                                .find(|p| &p.name == name)
                                .is_some_and(|p| p.is_out),
                            _ => {
                                let out = params.get(pos_idx).is_some_and(|p| p.is_out);
                                pos_idx += 1;
                                out
                            }
                        };
                        if !is_out {
                            self.maybe_consume_arg(arg, &fn_name, consumed);
                        }
                    }
                }
            }

            ExpressionKind::Member(obj, _) => {
                // Only check the object, not the field name expression.
                self.check_expr(obj, consumed);
            }

            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.check_expr(l, consumed);
                self.check_expr(r, consumed);
            }

            ExpressionKind::Unary(_, e) => {
                self.check_expr(e, consumed);
            }

            ExpressionKind::Assignment(lhs, _, rhs) => {
                self.check_expr(rhs, consumed);
                // Re-assigning a variable revives it (clears consumed status).
                if let LeftHandSideExpression::Identifier(name_expr) = lhs.as_ref() {
                    if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                        consumed.remove(name.as_str());
                    }
                }
            }

            ExpressionKind::Index(obj, idx) => {
                self.check_expr(obj, consumed);
                self.check_expr(idx, consumed);
            }

            ExpressionKind::Conditional(cond, then, else_, _) => {
                self.check_expr(cond, consumed);
                self.check_expr(then, consumed);
                if let Some(e) = else_ {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Range(start, end_, _) => {
                self.check_expr(start, consumed);
                if let Some(e) = end_ {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Guard(_, e) => {
                self.check_expr(e, consumed);
            }

            ExpressionKind::FormattedString(parts) => {
                for part in parts {
                    self.check_expr(part, consumed);
                }
            }

            ExpressionKind::List(elems)
            | ExpressionKind::Set(elems)
            | ExpressionKind::Tuple(elems) => {
                for e in elems {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Array(elems, _) => {
                for e in elems {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(k, consumed);
                    self.check_expr(v, consumed);
                }
            }

            ExpressionKind::Match(scrutinee, branches) => {
                self.check_expr(scrutinee, consumed);

                // Each arm is an independent execution path.  A variable is
                // "definitely consumed" after a match only if it is consumed in
                // every arm; otherwise we conservatively leave it alive.
                let pre_consumed = consumed.clone();
                let mut intersection: Option<HashMap<String, ConsumedInfo>> = None;

                for branch in branches {
                    let mut arm_consumed = pre_consumed.clone();
                    // §12.0.6: pattern bindings in this arm (`case Some(g): ...`)
                    // are locals — calling them is a dynamic-fn dispatch.  Add
                    // each branch's pattern-bound names to `fn_bindings` for the
                    // arm body, then restore so they do not leak across arms or
                    // past the match expression.
                    let prev_bindings = self.fn_bindings.clone();
                    for pat in &branch.patterns {
                        Self::collect_pattern_bindings(pat, &mut self.fn_bindings);
                    }
                    if let Some(guard) = &branch.guard {
                        self.check_expr(guard, &mut arm_consumed);
                    }
                    self.check_stmt(&branch.body, &mut arm_consumed);
                    self.fn_bindings = prev_bindings;

                    intersection = Some(match intersection.take() {
                        None => arm_consumed,
                        Some(acc) => acc
                            .into_iter()
                            .filter(|(k, _)| arm_consumed.contains_key(k))
                            .collect(),
                    });
                }

                if let Some(result) = intersection {
                    *consumed = result;
                }
            }

            ExpressionKind::EnumValue(name, values) => {
                self.check_expr(name, consumed);
                for v in values {
                    self.check_expr(v, consumed);
                }
            }

            ExpressionKind::NamedArgument(_, val) => {
                self.check_expr(val, consumed);
            }

            ExpressionKind::Lambda(lambda) => {
                // Lambda body gets its own consumed map (captures are borrowed, not consumed).
                // §12.0.6: lambda parameters are bindings inside the body — an
                // fn-typed lambda parameter called as `g(items)` must be
                // classified as a dynamic-fn callee.  Snapshot/restore so the
                // params do not leak into the surrounding scope after the
                // lambda expression is evaluated.
                let prev_bindings = self.fn_bindings.clone();
                for p in &lambda.params {
                    self.fn_bindings.insert(p.name.clone());
                }
                let mut lambda_consumed = HashMap::new();
                self.check_stmt(&lambda.body, &mut lambda_consumed);
                self.fn_bindings = prev_bindings;
            }

            ExpressionKind::Block(stmts, final_expr) => {
                for s in stmts {
                    self.check_stmt(s, consumed);
                }
                self.check_expr(final_expr, consumed);
            }

            // Leaf nodes: literals, type expressions, super — nothing to check.
            ExpressionKind::Literal(_)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Super => {}
        }
    }

    /// If `arg` is a plain `Identifier` (or a `NamedArgument` wrapping one) that refers
    /// to a variable that should be consumed, mark it as consumed by `fn_name`.
    ///
    /// At top level: any managed type is consumed (existing §7.1 behaviour).
    /// Inside a function body: only resource types (those with `fn drop(self)`) are consumed.
    fn maybe_consume_arg(
        &self,
        arg: &Expression,
        fn_name: &str,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        match &arg.node {
            ExpressionKind::Identifier(name, _)
                if !consumed.contains_key(name.as_str()) && self.should_consume_expr(arg) =>
            {
                consumed.insert(
                    name.clone(),
                    ConsumedInfo {
                        by_fn: fn_name.to_string(),
                        at_span: arg.span,
                    },
                );
            }
            ExpressionKind::NamedArgument(_, val) => {
                if let ExpressionKind::Identifier(name, _) = &val.node {
                    if !consumed.contains_key(name.as_str()) && self.should_consume_expr(val) {
                        consumed.insert(
                            name.clone(),
                            ConsumedInfo {
                                by_fn: fn_name.to_string(),
                                at_span: val.span,
                            },
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Returns true if passing this expression should consume it.
    ///
    /// Rule:
    /// - Resource types (`fn drop(self)` present): always consumed, at all call sites.
    /// - Other managed (non-auto-copy) types: consumed only at top level (§7.1 behaviour).
    /// - Inside a function body: only resource types are consumed.
    fn should_consume_expr(&self, expr: &Expression) -> bool {
        if let Some(ty) = self.types.get(&expr.id) {
            if is_resource(&ty.kind, self.type_definitions) {
                true
            } else if self.in_function_body {
                false
            } else {
                !is_auto_copy(&ty.kind, self.type_definitions)
            }
        } else {
            false
        }
    }

    /// §12.0.6 dynamic-fn fallback: at a call site whose callee is a dynamic
    /// fn-value, every non-auto-copy argument is consumed regardless of scope.
    /// Mirrors `maybe_consume_arg` but uses the broader "any managed type"
    /// predicate (since the conservative rule says "all managed-type params
    /// escape", not just resources).
    fn consume_arg_dynamic(
        &self,
        arg: &Expression,
        sink: &str,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let (name, ident_expr) = match &arg.node {
            ExpressionKind::Identifier(n, _) => (n.clone(), arg),
            ExpressionKind::NamedArgument(_, val) => match &val.node {
                ExpressionKind::Identifier(n, _) => (n.clone(), val.as_ref()),
                _ => return,
            },
            _ => return,
        };
        if consumed.contains_key(name.as_str()) {
            return;
        }
        // Auto-copy types pass by value at every call boundary; the dynamic-fn
        // rule still respects this (an auto-copy struct never aliases the
        // caller's heap and so cannot be retained by the callee).
        let Some(ty) = self.types.get(&ident_expr.id) else {
            return;
        };
        if is_auto_copy(&ty.kind, self.type_definitions) {
            return;
        }
        consumed.insert(
            name,
            ConsumedInfo {
                by_fn: sink.to_string(),
                at_span: ident_expr.span,
            },
        );
    }

    /// §12.0.6: Collect every identifier name introduced by a match pattern,
    /// adding it to `out` (the running `fn_bindings` set).  Recurses through
    /// `Tuple` and `EnumVariant` payloads.  `Member` patterns describe a
    /// *path* (`Color.Red`) — only their inner sub-pattern can introduce
    /// bindings, never the path itself.
    fn collect_pattern_bindings(pat: &Pattern, out: &mut HashSet<String>) {
        match pat {
            Pattern::Identifier(name) => {
                out.insert(name.clone());
            }
            Pattern::Tuple(parts) | Pattern::EnumVariant(_, parts) => {
                for p in parts {
                    Self::collect_pattern_bindings(p, out);
                }
            }
            Pattern::Member(inner, _) => Self::collect_pattern_bindings(inner, out),
            Pattern::Literal(_) | Pattern::Regex(_) | Pattern::Default => {}
        }
    }

    /// §12.0.6 dynamic-fn classifier — see `fn_bindings` field doc.
    ///
    /// Returns `true` when `callee` is a fn-typed value that the type checker
    /// cannot resolve to a literal callee name:
    ///
    /// - `Identifier(name, _)` where `name` is in `self.fn_bindings` (a
    ///   parameter or `let`/`var` local, never a top-level declaration).
    /// - `Conditional` / `Match` / `Block` / `Call` / `Lambda` / `Member` of a
    ///   non-method (the value of an expression) — anything that is not a
    ///   syntactic free-function reference.  Method calls go through
    ///   `extract_method_summary` and are explicitly excluded here.
    ///
    /// Free functions declared at module scope (`fn foo(...)`) are never
    /// re-bound as locals, so their identifier callees are absent from
    /// `fn_bindings` and treated as literal — preserving §7.4 behaviour for
    /// the in-function-body managed-arg path.
    fn is_dynamic_fn_callee(&self, callee: &Expression) -> bool {
        match &callee.node {
            ExpressionKind::Identifier(name, _) => self.fn_bindings.contains(name.as_str()),
            // Method calls dispatch through the §12.0.3 path; they are literal.
            ExpressionKind::Member(_, _) => false,
            // Any other callee shape produces an fn-value at runtime — dynamic.
            ExpressionKind::Conditional(_, _, _, _)
            | ExpressionKind::Match(_, _)
            | ExpressionKind::Block(_, _)
            | ExpressionKind::Call(_, _)
            | ExpressionKind::Lambda(_)
            | ExpressionKind::Index(_, _) => true,
            _ => false,
        }
    }

    fn extract_callee_name(&self, callee: &Expression) -> String {
        match &callee.node {
            ExpressionKind::Identifier(name, _) => name.clone(),
            ExpressionKind::Member(_, method) => {
                if let ExpressionKind::Identifier(name, _) = &method.node {
                    name.clone()
                } else {
                    "<method>".to_string()
                }
            }
            _ => "<function>".to_string(),
        }
    }

    // ── §12.0.3 Method / self semantics ──────────────────────────────────────

    /// Look up the escape summary for a static method call on `class_name`.
    ///
    /// Walks the `base_class` chain so that inherited methods are found in the
    /// defining class's summary, not the static type's (§12.0.3 inherited rule).
    fn lookup_static_method_summary(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<&EscapeSummary> {
        let key = format!("{class_name}_{method_name}");
        if let Some(s) = self.escape_summaries.get(&key) {
            return Some(s);
        }
        // Walk base_class chain for inherited methods.
        let mut current = class_name;
        loop {
            match self.type_definitions.get(current) {
                Some(TypeDefinition::Class(cd)) => match &cd.base_class {
                    Some(base) => {
                        let base_key = format!("{base}_{method_name}");
                        if let Some(s) = self.escape_summaries.get(&base_key) {
                            return Some(s);
                        }
                        current = base.as_str();
                    }
                    None => return None,
                },
                _ => return None,
            }
        }
    }

    /// Build a joined escape summary for a virtual / trait-dispatch method call.
    ///
    /// Collects all concrete classes that implement `trait_or_abstract` (directly
    /// or via inheritance) and unions their escape summaries (§12.0.3 virtual rule).
    ///
    /// Returns `None` when no implementers are visible — the caller falls back to
    /// the "every managed param escapes" conservative treatment per §12.0.6.
    fn join_virtual_summaries(
        &self,
        trait_or_abstract: &str,
        method_name: &str,
    ) -> Option<EscapeSummary> {
        let mut joined = EscapeSummary::default();
        let mut any_found = false;

        for (class_name, td) in self.type_definitions {
            let TypeDefinition::Class(cd) = td else {
                continue;
            };
            // Include concrete implementers only.
            if cd.is_abstract {
                continue;
            }
            let implements_trait = cd.traits.iter().any(|t| t == trait_or_abstract);
            let inherits_abstract =
                !implements_trait && self.class_extends(class_name, trait_or_abstract);
            if !implements_trait && !inherits_abstract {
                continue;
            }
            if let Some(s) = self.lookup_static_method_summary(class_name, method_name) {
                joined
                    .direct_escapes
                    .extend(s.direct_escapes.iter().copied());
                joined
                    .return_aliases
                    .extend(s.return_aliases.iter().copied());
                joined
                    .conditional_escapes
                    .extend(s.conditional_escapes.iter().cloned());
                any_found = true;
            }
        }

        if any_found {
            Some(joined)
        } else {
            None
        }
    }

    /// Returns `true` if `class_name` has `ancestor` anywhere in its `base_class` chain.
    fn class_extends(&self, class_name: &str, ancestor: &str) -> bool {
        let mut current = class_name;
        loop {
            if current == ancestor {
                return true;
            }
            match self.type_definitions.get(current) {
                Some(TypeDefinition::Class(cd)) => match &cd.base_class {
                    Some(base) => current = base.as_str(),
                    None => return false,
                },
                _ => return false,
            }
        }
    }

    /// If `callee` is a method call (`Member(receiver, method_name)`), looks up
    /// and returns the applicable escape summary for the method.
    ///
    /// Returns `None` when the callee is a free function, the receiver type is
    /// unresolved, or no summary is present for the method yet.
    fn extract_method_summary(&self, callee: &Expression) -> Option<EscapeSummary> {
        let ExpressionKind::Member(obj_expr, method_expr) = &callee.node else {
            return None;
        };
        let ExpressionKind::Identifier(method_name, _) = &method_expr.node else {
            return None;
        };
        let receiver_ty = self.types.get(&obj_expr.id)?;
        let TypeKind::Custom(type_name, _) = &receiver_ty.kind else {
            return None;
        };

        match self.type_definitions.get(type_name.as_str()) {
            Some(TypeDefinition::Trait(_)) => self.join_virtual_summaries(type_name, method_name),
            Some(TypeDefinition::Class(cd)) if cd.is_abstract => {
                self.join_virtual_summaries(type_name, method_name)
            }
            Some(TypeDefinition::Class(_)) => self
                .lookup_static_method_summary(type_name, method_name)
                .cloned(),
            Some(TypeDefinition::Struct(_))
            | Some(TypeDefinition::Enum(_))
            | Some(TypeDefinition::Generic(_))
            | Some(TypeDefinition::Alias(_))
            | None => None,
        }
    }
}
