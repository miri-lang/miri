// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Use-after-move analysis for the Miri type checker.
//!
//! Tracks which variables have been "consumed" and emits a compile-time error
//! if a consumed variable is subsequently accessed. A variable is consumed by:
//! - Passing it as an argument to a function call
//! - Assigning it to another variable when the type is a resource
//!
//! Auto-copy types (size ≤ 128 bytes, all-primitive fields) are always copied
//! and are never flagged. Resource types (`fn drop(self)` defined) are always
//! tracked; other managed types are only tracked at top-level scope.

use crate::ast::expression::{ExpressionKind, LeftHandSideExpression};
use crate::ast::pattern::Pattern;
use crate::ast::statement::{BindingResidency, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::diagnostic::{Diagnostic, Severity};
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};
use std::collections::{HashMap, HashSet};

use super::context::TypeDefinition;
use super::escape_analysis::{EscapeNextHop, EscapeSummary, FunctionId};
use super::utils::{is_auto_copy, is_resource};

#[derive(Clone)]
struct ConsumedInfo {
    by_fn: String,
    #[allow(dead_code)]
    at_span: Span,
    /// "consumed because:" chain lines, empty when no chain was computed.
    chain: Vec<String>,
}

pub struct UseAfterMoveChecker<'a> {
    types: &'a HashMap<usize, Type>,
    type_definitions: &'a HashMap<String, TypeDefinition>,
    /// Escape summaries keyed by `FunctionId` (`"fn_name"` or `"ClassName_method"`).
    /// Pre-populated with FFI summaries; user-method summaries added during analysis.
    escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    errors: Vec<TypeError>,
    warnings: Vec<Diagnostic>,
    /// True when analysis is inside a function body (vs. top-level script code).
    /// Resource types are always tracked. Managed types are only tracked at top level.
    in_function_body: bool,
    /// Names currently bound as parameters or `let`/`var` locals.  Used to
    /// distinguish a literal callee identifier (a free function declared at
    /// module scope — never re-bound, so absent here) from a dynamic fn-value
    /// bound to a name (let-bound, parameter, re-assigned).  A `Call` whose
    /// callee identifier is in this set is a dynamic call and consumes every
    /// managed-typed argument.
    ///
    /// The set is snapshotted on entry to a function body and restored on exit
    /// so an inner function does not see its caller's bindings.
    fn_bindings: HashSet<String>,
    /// Names currently bound as gpu-resident locals (`gpu let` / `gpu var`).
    /// A gpu binding owns a device buffer and is linear per residency (§6.5);
    /// `gpu let b = a` where `a` is gpu-resident is a move that consumes `a`
    /// (D24), whereas a cross-residency `let h = a` is a copy (D23). Snapshotted
    /// and restored alongside `fn_bindings`.
    gpu_bindings: HashSet<String>,
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
            gpu_bindings: HashSet::new(),
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
            StatementKind::FunctionDeclaration(decl) => self.check_fn_decl(decl),
            StatementKind::Block(stmts) => self.check_block(stmts, consumed),
            StatementKind::Expression(expr) => self.check_expr(expr, consumed),
            StatementKind::Variable(decls, _) => self.check_variable_stmt(decls, consumed),
            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.check_expr(e, consumed);
                }
            }
            StatementKind::If(cond, then, else_, _) => {
                self.check_if_stmt(cond, then, else_.as_deref(), consumed);
            }
            StatementKind::While(cond, body, _) => {
                self.check_expr(cond, consumed);
                self.check_stmt(body, consumed);
            }
            StatementKind::For(decls, iter, body) | StatementKind::GpuFor(decls, iter, body) => {
                self.check_for_stmt(decls, iter, body, consumed);
            }
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Use(_, _)
            | StatementKind::Type(_, _)
            | StatementKind::Enum(_, _, _, _, _, _)
            | StatementKind::Struct(_, _, _, _, _)
            | StatementKind::Class(_)
            | StatementKind::Trait(_, _, _, _, _)
            | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
            | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
        }
    }

    fn check_fn_decl(&mut self, decl: &crate::ast::statement::FunctionDeclarationData) {
        let Some(body) = &decl.body else { return };
        let prev_in_fn = self.in_function_body;
        let prev_bindings = std::mem::take(&mut self.fn_bindings);
        let prev_gpu = std::mem::take(&mut self.gpu_bindings);
        self.in_function_body = true;
        for p in &decl.params {
            self.fn_bindings.insert(p.name.clone());
        }
        let mut fn_consumed: HashMap<String, ConsumedInfo> = HashMap::new();
        self.check_stmt(body, &mut fn_consumed);
        self.in_function_body = prev_in_fn;
        self.fn_bindings = prev_bindings;
        self.gpu_bindings = prev_gpu;
    }

    fn check_variable_stmt(
        &mut self,
        decls: &[crate::ast::statement::VariableDeclaration],
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        for decl in decls {
            if let Some(init) = &decl.initializer {
                self.check_expr(init, consumed);
                if let ExpressionKind::Identifier(src, _) = &init.node {
                    let gpu_move =
                        decl.residency == BindingResidency::Gpu && self.gpu_bindings.contains(src);
                    let resource = self
                        .types
                        .get(&init.id)
                        .is_some_and(|ty| is_resource(&ty.kind, self.type_definitions));
                    if gpu_move || resource {
                        let by_fn = if gpu_move {
                            format!("move to gpu binding '{}'", decl.name)
                        } else {
                            format!("assignment to '{}'", decl.name)
                        };
                        consumed.insert(
                            src.clone(),
                            ConsumedInfo {
                                by_fn,
                                at_span: init.span,
                                chain: vec![],
                            },
                        );
                    }
                }
            }
            if decl.residency == BindingResidency::Gpu {
                self.gpu_bindings.insert(decl.name.clone());
            }
            self.fn_bindings.insert(decl.name.clone());
        }
    }

    fn check_if_stmt(
        &mut self,
        cond: &Expression,
        then: &Statement,
        else_: Option<&Statement>,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        self.check_expr(cond, consumed);
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

    fn check_for_stmt(
        &mut self,
        decls: &[crate::ast::statement::VariableDeclaration],
        iter: &Expression,
        body: &Statement,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        self.check_expr(iter, consumed);
        let prev_bindings = self.fn_bindings.clone();
        let prev_gpu = self.gpu_bindings.clone();
        for d in decls {
            self.fn_bindings.insert(d.name.clone());
        }
        self.check_stmt(body, consumed);
        self.fn_bindings = prev_bindings;
        self.gpu_bindings = prev_gpu;
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
    /// warning as `type 'T'`).
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
                self.report_use_after_consume(name, expr, consumed)
            }
            ExpressionKind::Call(callee, args) => self.check_call_expr(callee, args, consumed),
            ExpressionKind::Member(obj, _) => self.check_expr(obj, consumed),
            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.check_expr(l, consumed);
                self.check_expr(r, consumed);
            }
            ExpressionKind::Unary(_, e) => self.check_expr(e, consumed),
            ExpressionKind::Assignment(lhs, _, rhs) => {
                self.check_assignment_expr(lhs, rhs, consumed)
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
            ExpressionKind::Guard(_, e) => self.check_expr(e, consumed),
            ExpressionKind::FormattedString(parts)
            | ExpressionKind::List(parts)
            | ExpressionKind::Set(parts)
            | ExpressionKind::Tuple(parts) => self.check_each(parts, consumed),
            ExpressionKind::Array(elems, _) => self.check_each(elems, consumed),
            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(k, consumed);
                    self.check_expr(v, consumed);
                }
            }
            ExpressionKind::Match(scrutinee, branches) => {
                self.check_match_expr(scrutinee, branches, consumed);
            }
            ExpressionKind::EnumValue(name, values) => {
                self.check_expr(name, consumed);
                self.check_each(values, consumed);
            }
            ExpressionKind::NamedArgument(_, val) => self.check_expr(val, consumed),
            ExpressionKind::Lambda(lambda) => self.check_lambda_expr(lambda),
            ExpressionKind::Block(stmts, final_expr) => {
                for s in stmts {
                    self.check_stmt(s, consumed);
                }
                self.check_expr(final_expr, consumed);
            }
            ExpressionKind::Cast(value_expr, _target_type_expr) => {
                self.check_expr(value_expr, consumed);
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

    fn check_each(&mut self, exprs: &[Expression], consumed: &mut HashMap<String, ConsumedInfo>) {
        for e in exprs {
            self.check_expr(e, consumed);
        }
    }

    fn check_assignment_expr(
        &mut self,
        lhs: &LeftHandSideExpression,
        rhs: &Expression,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        self.check_expr(rhs, consumed);
        if let LeftHandSideExpression::Identifier(name_expr) = lhs {
            if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                consumed.remove(name.as_str());
            }
        }
    }

    fn check_lambda_expr(&mut self, lambda: &crate::ast::expression::LambdaData) {
        let prev_bindings = self.fn_bindings.clone();
        for p in &lambda.params {
            self.fn_bindings.insert(p.name.clone());
        }
        let mut lambda_consumed = HashMap::new();
        self.check_stmt(&lambda.body, &mut lambda_consumed);
        self.fn_bindings = prev_bindings;
    }

    fn report_use_after_consume(
        &mut self,
        name: &str,
        expr: &Expression,
        consumed: &HashMap<String, ConsumedInfo>,
    ) {
        if let Some(info) = consumed.get(name) {
            let chain_section = if info.chain.is_empty() {
                String::new()
            } else {
                format!("\n  consumed because:\n{}", info.chain.join("\n"))
            };
            self.errors.push(TypeError {
                kind: TypeErrorKind::Custom {
                    message: format!(
                        "'{}' was consumed by '{}' and cannot be used again{}\n  fix: call .clone() to keep your copy independent",
                        name, info.by_fn, chain_section
                    ),
                    help: None,
                },
                span: expr.span,
                source_override: None,
            });
        }
    }

    fn check_match_expr(
        &mut self,
        scrutinee: &Expression,
        branches: &[crate::ast::pattern::MatchBranch],
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        self.check_expr(scrutinee, consumed);

        let pre_consumed = consumed.clone();
        let mut intersection: Option<HashMap<String, ConsumedInfo>> = None;

        for branch in branches {
            let mut arm_consumed = pre_consumed.clone();
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

    fn check_call_expr(
        &mut self,
        callee: &Expression,
        args: &[Expression],
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let fn_name = self.extract_callee_name(callee);
        let method_chain_key: Option<String> = self.extract_method_chain_key(callee);
        let method_summary: Option<EscapeSummary> = self.extract_method_summary(callee);

        let free_fn_summary: Option<EscapeSummary> =
            if self.in_function_body && method_summary.is_none() {
                match &callee.node {
                    ExpressionKind::Identifier(name, _)
                        if !self.fn_bindings.contains(name.as_str()) =>
                    {
                        self.escape_summaries.get(name.as_str()).cloned()
                    }
                    _ => None,
                }
            } else {
                None
            };

        let is_dynamic_fn = method_summary.is_none()
            && free_fn_summary.is_none()
            && self.is_dynamic_fn_callee(callee);

        self.check_expr(callee, consumed);
        for arg in args {
            self.check_expr(arg, consumed);
        }

        let params: Vec<Parameter> = self
            .types
            .get(&callee.id)
            .and_then(|ty| {
                if let TypeKind::Function(fd) = &ty.kind {
                    Some(fd.params.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        if let Some(summary) = method_summary {
            let method_sink = method_chain_key.as_deref().unwrap_or(&fn_name);
            if summary.directly_escapes(0) {
                if let ExpressionKind::Member(obj_expr, _) = &callee.node {
                    self.consume_arg_dynamic(obj_expr, method_sink, 0, consumed);
                }
            }
            self.apply_summary_to_args(args, &params, &summary, method_sink, consumed, true);
        } else if let Some(summary) = free_fn_summary {
            self.apply_summary_to_args(args, &params, &summary, &fn_name, consumed, false);
        } else if is_dynamic_fn {
            let sink = format!("dynamic fn '{fn_name}'");
            self.consume_args_unconditional(args, &params, &sink, consumed);
        } else {
            self.consume_args_with_predicate(args, &params, &fn_name, consumed);
        }
    }

    fn arg_classify(arg: &Expression, params: &[Parameter], pos_idx: &mut usize) -> (bool, usize) {
        match &arg.node {
            ExpressionKind::NamedArgument(name, _) => {
                let out = params
                    .iter()
                    .find(|p| &p.name == name)
                    .is_some_and(|p| p.is_out);
                let idx = params
                    .iter()
                    .position(|p| &p.name == name)
                    .unwrap_or(usize::MAX);
                (out, idx)
            }
            _ => {
                let out = params.get(*pos_idx).is_some_and(|p| p.is_out);
                let idx = *pos_idx;
                *pos_idx += 1;
                (out, idx)
            }
        }
    }

    fn apply_summary_to_args(
        &self,
        args: &[Expression],
        params: &[Parameter],
        summary: &EscapeSummary,
        sink: &str,
        consumed: &mut HashMap<String, ConsumedInfo>,
        has_self_offset: bool,
    ) {
        let mut pos_idx = 0usize;
        for arg in args {
            let (is_out, mut idx) = Self::arg_classify(arg, params, &mut pos_idx);
            if has_self_offset && idx != usize::MAX {
                idx += 1;
            }
            if !is_out && summary.directly_escapes(idx) {
                self.consume_arg_dynamic(arg, sink, idx, consumed);
            }
        }
    }

    fn consume_args_unconditional(
        &self,
        args: &[Expression],
        params: &[Parameter],
        sink: &str,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let mut pos_idx = 0usize;
        for arg in args {
            let (is_out, idx) = Self::arg_classify(arg, params, &mut pos_idx);
            let param_idx = if idx == usize::MAX { 0 } else { idx };
            if !is_out {
                self.consume_arg_dynamic(arg, sink, param_idx, consumed);
            }
        }
    }

    fn consume_args_with_predicate(
        &self,
        args: &[Expression],
        params: &[Parameter],
        fn_name: &str,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let mut pos_idx = 0usize;
        for arg in args {
            let (is_out, idx) = Self::arg_classify(arg, params, &mut pos_idx);
            let param_idx = if idx == usize::MAX { 0 } else { idx };
            if !is_out {
                self.maybe_consume_arg(arg, fn_name, param_idx, consumed);
            }
        }
    }

    /// If `arg` is a plain `Identifier` (or a `NamedArgument` wrapping one) that refers
    /// to a variable that should be consumed, mark it as consumed by `fn_name`.
    ///
    /// At top level: any managed type is consumed.
    /// Inside a function body: only resource types (those with `fn drop(self)`) are consumed.
    ///
    /// `callee_param_idx` is the parameter index of this arg in `fn_name`'s parameter list,
    /// used to look up the escape chain for richer diagnostics.
    fn maybe_consume_arg(
        &self,
        arg: &Expression,
        fn_name: &str,
        callee_param_idx: usize,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        match &arg.node {
            ExpressionKind::Identifier(name, _)
                if !consumed.contains_key(name.as_str()) && self.should_consume_expr(arg) =>
            {
                let chain = self.build_chain(fn_name, callee_param_idx);
                consumed.insert(
                    name.clone(),
                    ConsumedInfo {
                        by_fn: fn_name.to_string(),
                        at_span: arg.span,
                        chain,
                    },
                );
            }
            ExpressionKind::NamedArgument(_, val) => {
                if let ExpressionKind::Identifier(name, _) = &val.node {
                    if !consumed.contains_key(name.as_str()) && self.should_consume_expr(val) {
                        let chain = self.build_chain(fn_name, callee_param_idx);
                        consumed.insert(
                            name.clone(),
                            ConsumedInfo {
                                by_fn: fn_name.to_string(),
                                at_span: val.span,
                                chain,
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
    /// - Other managed (non-auto-copy) types: consumed only at top level.
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

    /// Dynamic-fn fallback: at a call site whose callee is a dynamic fn-value,
    /// every non-auto-copy argument is consumed regardless of scope.
    /// Mirrors `maybe_consume_arg` but uses the broader "any managed type"
    /// predicate (since the conservative rule says "all managed-type params
    /// escape", not just resources).
    ///
    /// `callee_param_idx` is used for chain lookup when `sink` is a known callee name.
    fn consume_arg_dynamic(
        &self,
        arg: &Expression,
        sink: &str,
        callee_param_idx: usize,
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
        let chain = self.build_chain(sink, callee_param_idx);
        consumed.insert(
            name,
            ConsumedInfo {
                by_fn: sink.to_string(),
                at_span: ident_expr.span,
                chain,
            },
        );
    }

    /// Collect every identifier name introduced by a match pattern,
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

    /// Dynamic-fn classifier — see `fn_bindings` field doc.
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
    /// `fn_bindings` and treated as literal — preserving standard behaviour for
    /// the in-function-body managed-arg path.
    fn is_dynamic_fn_callee(&self, callee: &Expression) -> bool {
        match &callee.node {
            ExpressionKind::Identifier(name, _) => self.fn_bindings.contains(name.as_str()),
            // Method calls are handled elsewhere; they are literal callees.
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

    /// Builds the "consumed because:" chain lines by following `escape_next_hops`
    /// through the escape summaries.
    ///
    /// Starts at `callee_fn` / `param_idx` and follows hops until reaching a sink
    /// or a function with no hop data.  Returns the chain lines (each one line),
    /// or an empty Vec when no chain info is available.
    fn build_chain(&self, callee_fn: &str, param_idx: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_fn = callee_fn.to_string();
        let mut current_param = param_idx;
        let mut visited = std::collections::HashSet::new();

        loop {
            if !visited.insert((current_fn.clone(), current_param)) {
                break; // cycle guard
            }
            let Some(summary) = self.escape_summaries.get(&current_fn) else {
                break;
            };
            let Some(hop) = summary.escape_next_hops.get(&current_param) else {
                break;
            };
            match hop {
                EscapeNextHop::Call { callee, param_slot } => {
                    lines.push(format!(
                        "    {} → calls {} (passes its argument)",
                        current_fn, callee
                    ));
                    current_fn = callee.clone();
                    current_param = *param_slot;
                }
                EscapeNextHop::Return => {
                    lines.push(format!(
                        "    {} → returns its argument (escape sink)",
                        current_fn
                    ));
                    break;
                }
                EscapeNextHop::ReturnAggregate => {
                    lines.push(format!(
                        "    {} → returns its argument in an aggregate (escape sink)",
                        current_fn
                    ));
                    break;
                }
                EscapeNextHop::FieldStore { field } => {
                    lines.push(format!(
                        "    {} → stores its argument into field '{}' (escape sink)",
                        current_fn, field
                    ));
                    break;
                }
                EscapeNextHop::ClosureCapture => {
                    lines.push(format!(
                        "    {} → captures its argument in a returned closure (escape sink)",
                        current_fn
                    ));
                    break;
                }
                EscapeNextHop::DynamicFn { fn_name } => {
                    lines.push(format!(
                        "    {} → passes its argument to dynamic fn '{}' (escape sink)",
                        current_fn, fn_name
                    ));
                    break;
                }
            }
        }
        lines
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

    // ── Method / self semantics ─────────────────────────────────────────────────

    /// Look up the escape summary for a static method call on `class_name`.
    ///
    /// Walks the `base_class` chain so that inherited methods are found in the
    /// defining class's summary, not the static type's.
    fn lookup_static_method_summary(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<&EscapeSummary> {
        let owner = self.resolve_method_owner_class(class_name, method_name)?;
        self.escape_summaries.get(&format!("{owner}_{method_name}"))
    }

    /// Walk the `base_class` chain to find the class that owns a registered
    /// escape summary for `method_name`. Returns the first class whose
    /// `"ClassName_method"` key is present in `escape_summaries`, or `None`
    /// if no class in the chain has one.
    fn resolve_method_owner_class<'b>(
        &'b self,
        class_name: &'b str,
        method_name: &str,
    ) -> Option<&'b str> {
        let mut current = class_name;
        loop {
            let key = format!("{current}_{method_name}");
            if self.escape_summaries.contains_key(&key) {
                return Some(current);
            }
            match self.type_definitions.get(current) {
                Some(TypeDefinition::Class(cd)) => match &cd.base_class {
                    Some(base) => current = base.as_str(),
                    None => return None,
                },
                None
                | Some(TypeDefinition::Struct(_))
                | Some(TypeDefinition::Enum(_))
                | Some(TypeDefinition::Generic(_))
                | Some(TypeDefinition::Alias(_))
                | Some(TypeDefinition::Trait(_)) => return None,
            }
        }
    }

    /// Build a joined escape summary for a virtual / trait-dispatch method call.
    ///
    /// Collects all concrete classes that implement `trait_or_abstract` (directly
    /// or via inheritance) and unions their escape summaries.
    ///
    /// Returns `None` when no implementers are visible — the caller falls back to
    /// the "every managed param escapes" conservative treatment.
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

    /// Returns the qualified `"ClassName_method"` key for a method-call callee,
    /// resolved to the **defining** class — walks the `base_class` chain so an
    /// inherited method's chain key points at the class that actually has the
    /// escape summary (mirrors [`Self::lookup_static_method_summary`]).
    ///
    /// Used so `build_chain` can look up the escape summary by its canonical key
    /// rather than the bare method name, and so the user-visible "consumed by"
    /// label names the class that owns the field-store sink rather than the
    /// receiver's static type. Returns `None` for non-method callees or when
    /// the receiver type is unresolved.
    fn extract_method_chain_key(&self, callee: &Expression) -> Option<String> {
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
        let owner = self
            .resolve_method_owner_class(type_name, method_name)
            .unwrap_or(type_name);
        Some(format!("{owner}_{method_name}"))
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
