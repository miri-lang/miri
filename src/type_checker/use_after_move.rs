// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Use-after-move analysis for the Miri type checker.
//!
//! Tracks which variables have been "consumed" and emits a compile-time error
//! if a consumed variable is subsequently accessed. Only a resource (a value
//! whose release may run a `drop` hook) is ever consumed, and only by:
//! - a call whose callee keeps it, as the callee's escape summary says; a
//!   callee with no summary (a function value) is assumed to keep it;
//! - an assignment to another binding, a `return`, or `value.drop()`.
//!
//! Every other value is lent to a call and stays the caller's, at the top
//! level of a file as inside a function: the rule does not depend on scope.

use crate::ast::expression::{ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::{BindingResidency, StatementKind, WhileStatementType, DROP_HOOK_NAME};
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{Diagnostic, RelatedNote, Severity};
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};
use std::collections::{HashMap, HashSet};

use super::context::TypeDefinition;
use super::escape_analysis::{
    call_builds_a_value, collect_pattern_bindings, free_callee, method_owner, method_summary,
    returned_tails, EscapeNextHop, EscapeSummary, FunctionId,
};
use super::function_analysis::DeclaredFunction;
use super::utils::{hands_on_a_reference, is_resource, runs_drop_hook};
use crate::ast::captures::collect_lambda_captures;

/// Why `value.drop()` is refused on a receiver the scope does not own.
pub(crate) const BORROWED_DROP_MESSAGE: &str = "drop() can only release a value this scope owns";

/// How to release a value whose drop hook was called on a borrowed receiver.
pub(crate) const BORROWED_DROP_HELP: &str =
    "call drop() on the local variable that owns the value, or let its owner release it";

/// Whether `callee` names a GPU atomic built-in (`atomic_add`, …), which the
/// compiler lowers itself rather than calling a declared function.
fn is_gpu_atomic_builtin(callee: &Expression) -> bool {
    matches!(
        &callee.node,
        ExpressionKind::Identifier(name, _)
            if crate::gpu_target::GpuAtomicOp::from_builtin_name(name).is_some()
    )
}

/// What a consumed argument was handed to.
enum Sink {
    /// A function, whose escape summary can explain why the argument escapes.
    Function(FunctionId),
    /// A function value only known at run time, which has no summary.
    DynamicFn(String),
}

/// How a diagnostic names what consumed the argument.
impl std::fmt::Display for Sink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sink::Function(function) => function.fmt(f),
            Sink::DynamicFn(name) => write!(f, "dynamic fn '{name}'"),
        }
    }
}

/// The expressions that consume the binding they name and hand its reference
/// on: an argument a call keeps (a method's receiver included), and the
/// initializer of a binding a resource moves into. Each is recorded by the
/// expression naming the binding, and only when its type runs a drop hook of
/// its own: a value held at a trait is shared rather than moved, so consuming
/// it hands nothing on.
///
/// The one answer to "does consuming this binding release its reference
/// here": MIR lowering reads it to release the binding where it is consumed,
/// rather than where its scope ends. Lowering adds one rule of its own: a
/// parameter's reference is its caller's, so consuming a parameter releases
/// nothing in the callee.
#[derive(Debug, Default)]
pub struct ConsumedArguments {
    expressions: HashSet<usize>,
    /// The bindings each kept closure takes over, by the closure's expression.
    captures: HashMap<usize, Vec<String>>,
}

impl ConsumedArguments {
    /// Whether the call `argument` is passed to consumes the binding it names.
    pub fn consumes(&self, argument: &Expression) -> bool {
        self.expressions.contains(&argument.id)
    }

    /// The bindings the closure `lambda` creates takes over from the scope
    /// creating it: the resources it captures, when it is kept.
    pub fn captured_by(&self, lambda: &Expression) -> &[String] {
        self.captures.get(&lambda.id).map_or(&[], Vec::as_slice)
    }

    /// Add every argument `other` records.
    pub fn extend(&mut self, other: ConsumedArguments) {
        self.expressions.extend(other.expressions);
        self.captures.extend(other.captures);
    }
}

/// What the analysis finds in a program.
pub struct UseAfterMoveFindings {
    pub errors: Vec<TypeError>,
    pub warnings: Vec<Diagnostic>,
    pub consumed_arguments: ConsumedArguments,
}

/// The resources consumed on some path into a point, each with what
/// consumed it.
type Consumed = HashMap<String, ConsumedInfo>;

/// For each name a scope binds afresh, what the binding it shadows had
/// consumed when the scope bound it: the scope restores it when it ends.
type Shadowed = HashMap<String, Option<ConsumedInfo>>;

/// The control-flow question the analysis asks of the type checker: whether
/// any path reaches the point after a statement or an expression.
pub trait Reachability {
    /// Whether control can never reach the end of `stmt`.
    fn never_finishes(&self, stmt: &Statement) -> bool;
    /// Whether control can never reach the end of `expr`.
    fn never_ends(&self, expr: &Expression) -> bool;
}

impl Reachability for super::TypeChecker {
    fn never_finishes(&self, stmt: &Statement) -> bool {
        self.statement_never_finishes(stmt)
    }

    fn never_ends(&self, expr: &Expression) -> bool {
        self.expression_never_ends(expr, true)
    }
}

/// What the passes of the loop being analysed carry to its jumps: the state
/// at each `break`, which reaches the code after the loop, and at each
/// `continue`, which reaches the next pass.
#[derive(Default)]
struct LoopExits {
    breaks: Vec<Consumed>,
    continues: Vec<Consumed>,
}

/// What one pass of a loop runs: a condition before the body (`while`), the
/// body, and a condition after it (`do … while`). A `for` has neither: its
/// iterable is evaluated once, before the first pass.
#[derive(Clone, Copy)]
struct LoopPass<'e> {
    leading: Option<&'e Expression>,
    body: &'e Statement,
    trailing: Option<&'e Expression>,
}

/// Where one pass of a loop goes: the state it starts the next pass in, if
/// any path goes on, and the state on each path that leaves the loop.
struct PassOutcome {
    next: Option<Consumed>,
    leaves: Vec<Consumed>,
}

/// Add to `into` every resource `from` consumed: consumed on some path is
/// consumed after the paths meet.
fn merge_consumed(into: &mut Consumed, from: Consumed) {
    for (name, info) in from {
        into.entry(name).or_insert(info);
    }
}

/// [`merge_consumed`] from a state the caller keeps, copying only what
/// `into` lacks.
fn merge_consumed_from(into: &mut Consumed, from: &Consumed) {
    for (name, info) in from {
        if !into.contains_key(name) {
            into.insert(name.clone(), info.clone());
        }
    }
}

/// The state where the paths of `arms` meet: what any arm that reaches the
/// join consumed. An arm that never finishes reaches no join, and when no
/// arm does, the code after it is unreachable and keeps `before`.
fn join_arms(before: &Consumed, arms: impl IntoIterator<Item = (Consumed, bool)>) -> Consumed {
    let mut reaching = arms
        .into_iter()
        .filter_map(|(state, finishes)| finishes.then_some(state));
    let Some(mut joined) = reaching.next() else {
        return before.clone();
    };
    for state in reaching {
        merge_consumed(&mut joined, state);
    }
    joined
}

/// Remember, the first time `name` is bound afresh in a scope, what the
/// binding it shadows had consumed, and bind it unconsumed in `state`.
fn shadow(shadowed: &mut Shadowed, name: &str, state: &mut Consumed) {
    let outer = state.remove(name);
    if !shadowed.contains_key(name) {
        shadowed.insert(name.to_string(), outer);
    }
}

/// The scope that bound `shadowed` ends: each name names the outer binding
/// again, consumed or not as that one was.
fn restore_shadowed(state: &mut Consumed, shadowed: Shadowed) {
    for (name, outer) in shadowed {
        match outer {
            Some(info) => state.insert(name, info),
            None => state.remove(name.as_str()),
        };
    }
}

/// What identifies a reported error, so a loop's second pass reports only
/// what its first did not.
fn error_key(error: &TypeError) -> (Span, Option<DiagnosticCode>) {
    let code = if let TypeErrorKind::Coded { code, .. } = &error.kind {
        Some(*code)
    } else {
        None
    };
    (error.span, code)
}

#[derive(Clone)]
struct ConsumedInfo {
    by_fn: String,
    #[allow(dead_code)]
    at_span: Span,
    /// The "consumed because" chain, one note per step, each at the line
    /// that takes it when the report's file shows it; empty when no chain
    /// was computed.
    chain: Vec<RelatedNote>,
}

pub struct UseAfterMoveChecker<'a> {
    /// Answers whether control can reach the end of a statement or an
    /// expression.
    flow: &'a dyn Reachability,
    types: &'a HashMap<usize, Type>,
    type_definitions: &'a HashMap<String, TypeDefinition>,
    /// Escape summaries keyed by [`FunctionId`]: a free function by name, a
    /// method by its declaring type and name.
    /// Pre-populated with FFI summaries; user-method summaries added during analysis.
    escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    /// The function each free-function callee resolved to, by callee id: a
    /// free function's summary is its declaration's, never its name's.
    callees: &'a HashMap<usize, DeclaredFunction>,
    errors: Vec<TypeError>,
    warnings: Vec<Diagnostic>,
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
    /// A gpu binding owns a device buffer and is linear per residency.
    /// `gpu let b = a` where `a` is gpu-resident is a move that consumes `a`,
    /// whereas a cross-residency `let h = a` is a copy. Snapshotted and
    /// restored alongside `fn_bindings`.
    gpu_bindings: HashSet<String>,
    /// Names bound by a `let`/`var` of the function being analysed and not
    /// shadowed by a parameter, loop variable, match binding or lambda
    /// parameter. Only these values belong to the scope that names them, so
    /// only these may be released early with `value.drop()`. A lambda body
    /// starts empty: what it captures belongs to the scope that declared it.
    owned_locals: HashSet<String>,
    /// Every argument a call consumed, by the expression naming it.
    consumed_arguments: ConsumedArguments,
    /// The jumps of each loop enclosing the point analysed, innermost last;
    /// a function or lambda body starts with none.
    loop_exits: Vec<LoopExits>,
    /// Whether the pass being analysed re-checks a loop body from the state
    /// its next pass starts in. A loop inside such a pass runs once from
    /// that state: its own loop-carried uses were checked when the enclosing
    /// loop's first pass reached it, so re-checking them again would cost
    /// a walk for every combination of passes of the loops around it.
    rechecking: bool,
    /// The closures whose bodies enclose the point analysed, innermost last:
    /// what each captures, and the resources among them it was seen to read.
    closures: Vec<ClosureCaptures>,
    /// The resources a closure passed straight to a call captured, by the
    /// closure's expression: the call consumes them only if it keeps the
    /// closure, and lends them otherwise.
    lent_closures: HashMap<usize, Vec<Capture>>,
    /// The expressions the function being analysed returns without `return`
    /// ([`returned_tails`]), by id.
    tail_values: HashSet<usize>,
}

/// A resource a closure captures.
struct Capture {
    name: String,
    /// Where the closure's body first reads it.
    span: Span,
    /// Whether taking it over hands its reference on (see
    /// `hands_on_its_reference`).
    hands_on: bool,
}

/// What one closure captures from the scope that creates it.
struct ClosureCaptures {
    /// Every enclosing-scope name its body references.
    names: HashSet<String>,
    /// The captured resources.
    resources: Vec<Capture>,
}

impl<'a> UseAfterMoveChecker<'a> {
    pub fn new(
        flow: &'a dyn Reachability,
        types: &'a HashMap<usize, Type>,
        type_definitions: &'a HashMap<String, TypeDefinition>,
        escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
        callees: &'a HashMap<usize, DeclaredFunction>,
    ) -> Self {
        Self {
            flow,
            types,
            type_definitions,
            escape_summaries,
            callees,
            errors: vec![],
            warnings: vec![],
            fn_bindings: HashSet::new(),
            gpu_bindings: HashSet::new(),
            owned_locals: HashSet::new(),
            consumed_arguments: ConsumedArguments::default(),
            loop_exits: Vec::new(),
            rechecking: false,
            closures: Vec::new(),
            lent_closures: HashMap::new(),
            tail_values: HashSet::new(),
        }
    }

    /// Runs the analysis on a complete program and returns what it found.
    pub fn check_program(mut self, program: &Program) -> UseAfterMoveFindings {
        let mut consumed: HashMap<String, ConsumedInfo> = HashMap::new();
        self.check_block(&program.body, &mut consumed);
        UseAfterMoveFindings {
            errors: self.errors,
            warnings: self.warnings,
            consumed_arguments: self.consumed_arguments,
        }
    }

    fn check_stmt(&mut self, stmt: &Statement, consumed: &mut HashMap<String, ConsumedInfo>) {
        match &stmt.node {
            StatementKind::FunctionDeclaration(decl) => self.check_fn_decl(decl),
            StatementKind::Block(stmts) => self.check_block(stmts, consumed),
            StatementKind::Expression(expr) => {
                self.check_expr(expr, consumed);
                if self.tail_values.contains(&expr.id) {
                    self.consume_returned_resource(expr, consumed);
                }
            }
            StatementKind::Variable(decls, _) => self.check_variable_stmt(decls, consumed),
            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.check_expr(e, consumed);
                    self.consume_returned_resource(e, consumed);
                }
            }
            StatementKind::If(cond, then, else_, _) => {
                self.check_if_stmt(cond, then, else_.as_deref(), consumed);
            }
            StatementKind::While(cond, body, kind) => {
                self.check_while_stmt(cond, body, kind, consumed);
            }
            StatementKind::For(decls, iter, body) | StatementKind::GpuFrame(decls, iter, body) => {
                self.check_for_stmt(decls, iter, body, consumed);
            }
            StatementKind::Forall {
                vars: decls,
                iterable: iter,
                body,
                ..
            } => {
                self.check_for_stmt(decls, iter, body, consumed);
            }
            StatementKind::GpuFrameBlock(block) => {
                self.check_stmt(block, consumed);
            }
            StatementKind::Class(class_data) => self.check_method_bodies(&class_data.body),
            StatementKind::Struct(_, _, _, methods, _, _)
            | StatementKind::Enum(_, _, _, methods, _, _)
            | StatementKind::Trait(_, _, _, methods, _) => self.check_method_bodies(methods),
            StatementKind::Break => self.note_loop_exit(consumed, |exits| &mut exits.breaks),
            StatementKind::Continue => self.note_loop_exit(consumed, |exits| &mut exits.continues),
            StatementKind::Empty
            | StatementKind::Use(_, _)
            | StatementKind::Type(_, _)
            | StatementKind::RuntimeFunctionDeclaration(_, _, _, _, _)
            | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
        }
    }

    /// Analyses each method of a type declaration as a function body. A method's
    /// receiver is a parameter like any other, so `self` is never owned there.
    fn check_method_bodies(&mut self, members: &[Statement]) {
        for member in members {
            if let StatementKind::FunctionDeclaration(decl) = &member.node {
                self.check_fn_decl(decl);
            }
        }
    }

    fn check_fn_decl(&mut self, decl: &crate::ast::statement::FunctionDeclarationData) {
        let Some(body) = &decl.body else { return };
        let prev_bindings = std::mem::take(&mut self.fn_bindings);
        let prev_gpu = std::mem::take(&mut self.gpu_bindings);
        let prev_owned = std::mem::take(&mut self.owned_locals);
        let prev_closures = std::mem::take(&mut self.closures);
        let tails = returned_tails(decl.return_type.as_deref(), body);
        let prev_tails = std::mem::replace(
            &mut self.tail_values,
            tails.iter().map(|tail| tail.id).collect(),
        );
        for p in &decl.params {
            self.fn_bindings.insert(p.name.clone());
        }
        let prev_loops = std::mem::take(&mut self.loop_exits);
        let mut fn_consumed: HashMap<String, ConsumedInfo> = HashMap::new();
        self.check_stmt(body, &mut fn_consumed);
        self.closures = prev_closures;
        self.tail_values = prev_tails;
        self.loop_exits = prev_loops;
        self.fn_bindings = prev_bindings;
        self.gpu_bindings = prev_gpu;
        self.owned_locals = prev_owned;
    }

    /// A resource returned by name, after `return` or as a tail value the
    /// body ends on, moves to the caller, which owns it from then on, so the
    /// scope it was declared in ends without it — as it does for one assigned
    /// to another binding.
    fn consume_returned_resource(
        &mut self,
        returned: &Expression,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let ExpressionKind::Identifier(name, _) = &returned.node else {
            return;
        };
        let is_resource = self
            .types
            .get(&returned.id)
            .is_some_and(|ty| is_resource(&ty.kind, self.type_definitions));
        if is_resource && !self.gives_away_a_capture(name, returned.span) {
            consumed.insert(
                name.clone(),
                ConsumedInfo {
                    by_fn: "return".to_string(),
                    at_span: returned.span,
                    chain: vec![],
                },
            );
        }
    }

    /// Check each declaration's initializer, then bind its name afresh: the
    /// new binding is not consumed.
    fn check_variable_stmt(
        &mut self,
        decls: &[crate::ast::statement::VariableDeclaration],
        consumed: &mut Consumed,
    ) {
        for decl in decls {
            self.check_initializer(decl, consumed);
            consumed.remove(decl.name.as_str());
            self.bind_declared_name(decl);
        }
    }

    /// Check `decl`'s initializer; one that names a resource, or a gpu
    /// binding bound to another gpu binding, moves it to the new binding.
    fn check_initializer(
        &mut self,
        decl: &crate::ast::statement::VariableDeclaration,
        consumed: &mut Consumed,
    ) {
        let Some(init) = &decl.initializer else {
            return;
        };
        self.check_expr(init, consumed);
        let ExpressionKind::Identifier(src, _) = &init.node else {
            return;
        };
        let gpu_move = decl.residency == BindingResidency::Gpu && self.gpu_bindings.contains(src);
        if !gpu_move && !self.is_resource_expr(init) {
            return;
        }
        if !gpu_move && self.gives_away_a_capture(src, init.span) {
            return;
        }
        let by_fn = if gpu_move {
            format!("move to gpu binding '{}'", decl.name)
        } else {
            format!("assignment to '{}'", decl.name)
        };
        let info = ConsumedInfo {
            by_fn,
            at_span: init.span,
            chain: vec![],
        };
        consumed.insert(src.clone(), info);
        if self.hands_on_its_reference(init) {
            self.consumed_arguments.expressions.insert(init.id);
        }
    }

    /// Record `decl`'s name as a binding of the scope being analysed.
    fn bind_declared_name(&mut self, decl: &crate::ast::statement::VariableDeclaration) {
        if decl.residency == BindingResidency::Gpu {
            self.gpu_bindings.insert(decl.name.clone());
        }
        self.fn_bindings.insert(decl.name.clone());
        self.owned_locals.insert(decl.name.clone());
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
        let arms = [
            (then_consumed, self.finishes(then)),
            (else_consumed, else_.is_none_or(|e| self.finishes(e))),
        ];
        *consumed = join_arms(consumed, arms);
    }

    /// Whether control can reach the end of `stmt`.
    fn finishes(&self, stmt: &Statement) -> bool {
        !self.flow.never_finishes(stmt)
    }

    /// Record the state at a `break` or `continue` with the innermost loop.
    fn note_loop_exit(
        &mut self,
        consumed: &Consumed,
        jumps: impl FnOnce(&mut LoopExits) -> &mut Vec<Consumed>,
    ) {
        if let Some(exits) = self.loop_exits.last_mut() {
            jumps(exits).push(consumed.clone());
        }
    }

    /// A `while` or `until` tests its condition before every pass, a
    /// `do … while` after every pass.
    fn check_while_stmt(
        &mut self,
        cond: &Expression,
        body: &Statement,
        kind: &WhileStatementType,
        consumed: &mut Consumed,
    ) {
        let pass = match kind {
            WhileStatementType::While | WhileStatementType::Until | WhileStatementType::Forever => {
                LoopPass {
                    leading: Some(cond),
                    body,
                    trailing: None,
                }
            }
            WhileStatementType::DoWhile | WhileStatementType::DoUntil => LoopPass {
                leading: None,
                body,
                trailing: Some(cond),
            },
        };
        self.check_loop(pass, &HashSet::new(), consumed);
    }

    /// Check one pass of a loop, then the next pass again from the state it
    /// starts in: what the first pass consumed on every path that goes on,
    /// the end of the body or a `continue`. A resource the loop consumes and
    /// uses again is used after it was consumed on the pass before. A pass
    /// that leaves the loop (`break`, `return`) right after consuming starts
    /// no next pass.
    ///
    /// After the loop, what was consumed on any path out of it is consumed:
    /// a failed condition, the end of a pass that goes on, or a `break`.
    /// Only the second pass's new use-after-consume errors are kept; anything
    /// else it reports the first pass already did.
    fn check_loop(
        &mut self,
        pass: LoopPass<'_>,
        loop_variables: &HashSet<String>,
        consumed: &mut Consumed,
    ) {
        let before = std::mem::take(consumed);
        let mut declared: Vec<String> = loop_variables.iter().cloned().collect();
        names_declared_in(pass.body, &mut declared);
        declared.retain(|name| !before.contains_key(name));
        let PassOutcome { mut next, leaves } = self.check_pass(pass, before.clone());
        if let Some(next) = next.as_mut() {
            forget_names(next, &declared);
            if !self.rechecking && next.keys().any(|name| !before.contains_key(name)) {
                let mut entry = next.clone();
                merge_consumed_from(&mut entry, &before);
                self.check_next_pass(pass, entry);
            }
        }
        let mut after = before;
        for state in leaves.into_iter().chain(next) {
            merge_consumed(&mut after, state);
        }
        forget_names(&mut after, &declared);
        *consumed = after;
    }

    /// Check one pass of a loop from `state`: the leading condition, the
    /// body, and, on the paths that go on, the trailing condition.
    fn check_pass(&mut self, pass: LoopPass<'_>, mut state: Consumed) -> PassOutcome {
        let mut leaves = Vec::new();
        if let Some(test) = pass.leading {
            self.check_expr(test, &mut state);
            leaves.push(state.clone());
        }
        self.loop_exits.push(LoopExits::default());
        self.check_stmt(pass.body, &mut state);
        let exits = self.loop_exits.pop().unwrap_or_default();
        leaves.extend(exits.breaks);
        let mut going_on = exits.continues;
        if self.finishes(pass.body) {
            going_on.push(state);
        }
        let mut next = going_on.into_iter().reduce(|mut joined, state| {
            merge_consumed(&mut joined, state);
            joined
        });
        if let (Some(test), Some(next)) = (pass.trailing, next.as_mut()) {
            self.check_expr(test, next);
            leaves.push(next.clone());
        }
        PassOutcome { next, leaves }
    }

    /// Check `pass` once more from `entry`, the state a second pass starts
    /// in, keeping only the use-after-consume errors the first pass did not
    /// report and none of its warnings.
    fn check_next_pass(&mut self, pass: LoopPass<'_>, entry: Consumed) {
        let (errors_before, warnings_before) = (self.errors.len(), self.warnings.len());
        let rechecking = std::mem::replace(&mut self.rechecking, true);
        self.check_pass(pass, entry);
        self.rechecking = rechecking;
        self.warnings.truncate(warnings_before);
        let mut seen: HashSet<_> = self.errors[..errors_before].iter().map(error_key).collect();
        let new_errors: Vec<TypeError> = self
            .errors
            .drain(errors_before..)
            .filter(|error| seen.insert(error_key(error)))
            .collect();
        self.errors.extend(new_errors);
    }

    fn check_for_stmt(
        &mut self,
        decls: &[crate::ast::statement::VariableDeclaration],
        iter: &Expression,
        body: &Statement,
        consumed: &mut Consumed,
    ) {
        self.check_expr(iter, consumed);
        let prev_bindings = self.fn_bindings.clone();
        let prev_gpu = self.gpu_bindings.clone();
        let prev_owned = self.owned_locals.clone();
        for d in decls {
            self.fn_bindings.insert(d.name.clone());
            self.owned_locals.remove(d.name.as_str());
        }
        let loop_variables: HashSet<String> = decls.iter().map(|d| d.name.clone()).collect();
        let mut shadowed = Shadowed::new();
        for name in &loop_variables {
            shadow(&mut shadowed, name, consumed);
        }
        let pass = LoopPass {
            leading: None,
            body,
            trailing: None,
        };
        self.check_loop(pass, &loop_variables, consumed);
        restore_shadowed(consumed, shadowed);
        self.fn_bindings = prev_bindings;
        self.gpu_bindings = prev_gpu;
        self.owned_locals = prev_owned;
    }

    /// Processes a block's statement list with scope-exit warning for unconsumed resource vars.
    ///
    /// Variables declared at THIS block level (direct `StatementKind::Variable` stmts) are
    /// tracked. If a resource-typed variable is still live at the end of the block, a W0004
    /// warning is emitted — the drop hook still runs via RC. When the block ends, a name it
    /// declared names the binding it shadowed again, consumed or not as that one was.
    fn check_block(&mut self, stmts: &[Statement], consumed: &mut Consumed) {
        // A local declared in this block stops naming an owned value when the
        // block ends, and a name it shadowed names the outer binding again.
        let prev_owned = self.owned_locals.clone();
        let mut scope_resources: Vec<(String, String, Span)> = Vec::new();
        let mut shadowed = Shadowed::new();
        for s in stmts {
            let StatementKind::Variable(decls, _) = &s.node else {
                self.check_stmt(s, consumed);
                continue;
            };
            scope_resources.extend(self.declared_resources(decls, s.span));
            for decl in decls {
                self.check_initializer(decl, consumed);
                shadow(&mut shadowed, &decl.name, consumed);
                self.bind_declared_name(decl);
            }
        }
        self.owned_locals = prev_owned;
        self.warn_unconsumed_resources(&scope_resources, consumed);
        restore_shadowed(consumed, shadowed);
    }

    /// The resources `decls` declare, each with its type's name and `span`.
    fn declared_resources(
        &self,
        decls: &[crate::ast::statement::VariableDeclaration],
        span: Span,
    ) -> Vec<(String, String, Span)> {
        decls
            .iter()
            .filter_map(|decl| {
                let ty = self.types.get(&decl.initializer.as_ref()?.id)?;
                if !is_resource(&ty.kind, self.type_definitions) {
                    return None;
                }
                let type_name = Self::resource_type_name(&ty.kind)?;
                Some((decl.name.clone(), type_name.to_string(), span))
            })
            .collect()
    }

    /// Warn for each resource of `scope_resources` its scope ends without
    /// consuming.
    fn warn_unconsumed_resources(
        &mut self,
        scope_resources: &[(String, String, Span)],
        consumed: &Consumed,
    ) {
        for (var_name, type_name, span) in scope_resources {
            if !consumed.contains_key(var_name.as_str()) {
                self.warnings.push(Diagnostic {
                    severity: Severity::Warning,
                    code: Some("MER_OWN_001"),
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
                    expected: None,
                    actual: None,
                    repair: None,
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
            TypeKind::Generic(name, _) => Some(name.as_str()),
            _ => None,
        }
    }

    fn check_expr(&mut self, expr: &Expression, consumed: &mut HashMap<String, ConsumedInfo>) {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => {
                self.note_capture(name, expr);
                self.report_use_after_consume(name, expr.span, consumed)
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
            ExpressionKind::Conditional(then, cond, else_, _) => {
                self.check_conditional_expr(then, cond, else_.as_deref(), consumed)
            }
            ExpressionKind::Range(start, end_, _) => {
                self.check_expr(start, consumed);
                if let Some(e) = end_ {
                    self.check_expr(e, consumed);
                }
            }
            ExpressionKind::Guard(_, e) => self.check_expr(e, consumed),
            ExpressionKind::FormattedString(parts, _)
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
            ExpressionKind::Lambda(lambda) => {
                let captured = self.check_lambda_expr(lambda);
                self.consume_captures(expr.id, &captured, consumed);
            }
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

    /// `then if cond else else_`: the condition runs first, then one branch,
    /// and what either branch consumed is consumed after it.
    fn check_conditional_expr(
        &mut self,
        then: &Expression,
        cond: &Expression,
        else_: Option<&Expression>,
        consumed: &mut Consumed,
    ) {
        self.check_expr(cond, consumed);
        let mut then_consumed = consumed.clone();
        self.check_expr(then, &mut then_consumed);
        let mut else_consumed = consumed.clone();
        if let Some(e) = else_ {
            self.check_expr(e, &mut else_consumed);
        }
        let then_ends = !self.flow.never_ends(then);
        let else_ends = else_.is_none_or(|e| !self.flow.never_ends(e));
        *consumed = join_arms(
            consumed,
            [(then_consumed, then_ends), (else_consumed, else_ends)],
        );
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

    /// Checks a closure's body, which does not own what it captures, and
    /// returns the resources it captures.
    fn check_lambda_expr(&mut self, lambda: &crate::ast::expression::LambdaData) -> Vec<Capture> {
        self.closures.push(ClosureCaptures {
            names: collect_lambda_captures(lambda),
            resources: Vec::new(),
        });
        let prev_bindings = self.fn_bindings.clone();
        let prev_owned = std::mem::take(&mut self.owned_locals);
        for p in &lambda.params {
            self.fn_bindings.insert(p.name.clone());
        }
        let prev_loops = std::mem::take(&mut self.loop_exits);
        let mut lambda_consumed = HashMap::new();
        self.check_stmt(&lambda.body, &mut lambda_consumed);
        self.loop_exits = prev_loops;
        self.fn_bindings = prev_bindings;
        self.owned_locals = prev_owned;
        self.closures
            .pop()
            .map_or_else(Vec::new, |closure| closure.resources)
    }

    /// Records `name` as a resource the innermost closure captures, when the
    /// closure's body reads it from the scope that creates the closure.
    fn note_capture(&mut self, name: &str, expr: &Expression) {
        let is_resource = self.is_resource_expr(expr);
        let hands_on = self.hands_on_its_reference(expr);
        let Some(closure) = self.closures.last_mut() else {
            return;
        };
        let first_read = !closure.resources.iter().any(|seen| seen.name == name);
        if is_resource && first_read && closure.names.contains(name) {
            closure.resources.push(Capture {
                name: name.to_string(),
                span: expr.span,
                hands_on,
            });
        }
    }

    /// A kept closure holds what it captures, so creating it consumes each
    /// captured resource; capturing one already consumed is a use of it.
    /// Records each one the closure `lambda` takes over for lowering, which
    /// ends the binding where the closure is created.
    fn consume_captures(&mut self, lambda: usize, captured: &[Capture], consumed: &mut Consumed) {
        for capture in captured {
            if self.gives_away_a_capture(&capture.name, capture.span) {
                continue;
            }
            if consumed.contains_key(&capture.name) {
                self.report_use_after_consume(&capture.name, capture.span, consumed);
                continue;
            }
            if capture.hands_on {
                let taken = self.consumed_arguments.captures.entry(lambda).or_default();
                taken.push(capture.name.clone());
            }
            consumed.insert(
                capture.name.clone(),
                ConsumedInfo {
                    by_fn: "a closure that captures it".to_string(),
                    at_span: capture.span,
                    chain: vec![],
                },
            );
        }
    }

    /// Refuses handing on `name` when it is a capture of the closure whose
    /// body is analysed: the closure does not own what it captures, so it
    /// cannot keep, move, return or capture it into a closure it keeps.
    fn gives_away_a_capture(&mut self, name: &str, span: Span) -> bool {
        let captured = self
            .closures
            .last()
            .is_some_and(|closure| closure.names.contains(name));
        if captured {
            self.errors.push(given_away_capture_error(name, span));
        }
        captured
    }

    fn report_use_after_consume(
        &mut self,
        name: &str,
        span: Span,
        consumed: &HashMap<String, ConsumedInfo>,
    ) {
        if let Some(info) = consumed.get(name) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::Coded {
                    code: DiagnosticCode::OwnUseOfMovedValue,
                    message: format!(
                        "'{}' was consumed by '{}' and cannot be used again\n  fix: use '{}' before the call that keeps it, or pass it to a function that only reads it",
                        name, info.by_fn, name
                    ),
                    help: None,
                    expected: None,
                    actual: None,
                },
                span,
                source_override: None,
                repair: None,
                related: info.chain.clone(),
            });
        }
    }

    /// Each arm starts from the state after the scrutinee. A name an arm's
    /// pattern binds is the arm's own: unconsumed while the arm runs, and
    /// naming the outer binding again, consumed or not as it was, after it.
    fn check_match_expr(
        &mut self,
        scrutinee: &Expression,
        branches: &[crate::ast::pattern::MatchBranch],
        consumed: &mut Consumed,
    ) {
        self.check_expr(scrutinee, consumed);
        let arms: Vec<(Consumed, bool)> = branches
            .iter()
            .map(|branch| self.check_match_arm(branch, consumed))
            .collect();
        *consumed = join_arms(consumed, arms);
    }

    /// Check one arm of a match from `before`, returning the state at its
    /// end and whether control reaches it.
    fn check_match_arm(
        &mut self,
        branch: &crate::ast::pattern::MatchBranch,
        before: &Consumed,
    ) -> (Consumed, bool) {
        let mut arm_consumed = before.clone();
        let prev_bindings = self.fn_bindings.clone();
        let prev_owned = self.owned_locals.clone();
        let mut arm_bindings = HashSet::new();
        for pat in &branch.patterns {
            collect_pattern_bindings(pat, &mut arm_bindings);
        }
        let mut shadowed = Shadowed::new();
        for name in &arm_bindings {
            shadow(&mut shadowed, name, &mut arm_consumed);
        }
        self.owned_locals
            .retain(|name| !arm_bindings.contains(name));
        self.fn_bindings.extend(arm_bindings);
        if let Some(guard) = &branch.guard {
            self.check_expr(guard, &mut arm_consumed);
        }
        self.check_stmt(&branch.body, &mut arm_consumed);
        self.fn_bindings = prev_bindings;
        self.owned_locals = prev_owned;
        restore_shadowed(&mut arm_consumed, shadowed);
        (arm_consumed, self.finishes(&branch.body))
    }

    fn check_call_expr(
        &mut self,
        callee: &Expression,
        args: &[Expression],
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        if let Some(receiver) = self.drop_hook_receiver(callee, args) {
            self.check_drop_hook_call(receiver, consumed);
            return;
        }
        let fn_name = self.extract_callee_name(callee);
        let method_chain_key: Option<FunctionId> = self.extract_method_chain_key(callee);
        let method_summary: Option<EscapeSummary> = self.extract_method_summary(callee);

        let free_fn_summary: Option<EscapeSummary> = if method_summary.is_none() {
            match &callee.node {
                ExpressionKind::Identifier(name, _)
                    if !self.fn_bindings.contains(name.as_str()) =>
                {
                    let declared = free_callee(callee, name, self.callees);
                    self.escape_summaries.get(&declared).cloned()
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
            self.check_argument(arg, consumed);
        }

        // A GPU atomic built-in writes through the buffer it is handed and
        // keeps nothing, so the buffer stays usable after the call.
        if method_summary.is_none() && free_fn_summary.is_none() && is_gpu_atomic_builtin(callee) {
            return;
        }

        let consumed_before: HashSet<String> = consumed.keys().cloned().collect();
        self.consume_call_arguments(
            callee,
            args,
            &fn_name,
            (method_summary, method_chain_key, free_fn_summary),
            is_dynamic_fn,
            consumed,
        );
        self.refuse_a_kept_resource_passed_again(
            callee,
            args,
            &fn_name,
            &consumed_before,
            consumed,
        );
    }

    /// Checks one argument. A closure passed straight to the call lends what it
    /// captures until the call is seen to keep it.
    fn check_argument(&mut self, arg: &Expression, consumed: &mut Consumed) {
        let value = argument_value(arg);
        if let ExpressionKind::Lambda(lambda) = &value.node {
            let captured = self.check_lambda_expr(lambda);
            for capture in &captured {
                self.report_use_after_consume(&capture.name, capture.span, consumed);
            }
            self.lent_closures.insert(value.id, captured);
        } else {
            self.check_expr(arg, consumed);
        }
    }

    /// Consumes every resource argument the callee keeps, as its escape
    /// summary says, or every one when the checker cannot see the callee.
    fn consume_call_arguments(
        &mut self,
        callee: &Expression,
        args: &[Expression],
        fn_name: &str,
        summaries: (
            Option<EscapeSummary>,
            Option<FunctionId>,
            Option<EscapeSummary>,
        ),
        is_dynamic_fn: bool,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let (method_summary, method_chain_key, free_fn_summary) = summaries;
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

        let named_sink = || Sink::Function(self.named_callee(callee, fn_name));
        if let Some(summary) = method_summary {
            let method_sink = method_chain_key.map_or_else(named_sink, Sink::Function);
            if summary.directly_escapes(0) {
                if let ExpressionKind::Member(obj_expr, _) = &callee.node {
                    self.consume_resource_arg(obj_expr, &method_sink, 0, consumed);
                }
            }
            self.apply_summary_to_args(args, &params, &summary, &method_sink, consumed, true);
        } else if let Some(summary) = free_fn_summary {
            self.apply_summary_to_args(args, &params, &summary, &named_sink(), consumed, false);
        } else if call_builds_a_value(callee, self.type_definitions) {
            // The value a constructor builds holds every argument it is handed.
            self.consume_resource_args(args, &params, &named_sink(), consumed);
        } else {
            // A callee whose body the checker cannot see is assumed to keep
            // every resource it is handed.
            let sink = if is_dynamic_fn {
                Sink::DynamicFn(fn_name.to_string())
            } else {
                named_sink()
            };
            self.consume_resource_args(args, &params, &sink, consumed);
        }
    }

    /// Refuses a resource this call consumes that the call is also handed at
    /// another position, kept or lent. A resource has one owner, so the call
    /// cannot keep it twice, and it cannot lend a view of the value it keeps.
    /// Reported at the second position it is passed at.
    fn refuse_a_kept_resource_passed_again(
        &mut self,
        callee: &Expression,
        args: &[Expression],
        fn_name: &str,
        consumed_before: &HashSet<String>,
        consumed: &HashMap<String, ConsumedInfo>,
    ) {
        let receiver = match &callee.node {
            ExpressionKind::Member(object, _) => Some(object.as_ref()),
            _ => None,
        };
        let mut seen: HashSet<&str> = HashSet::new();
        for passed in receiver.into_iter().chain(args.iter().map(argument_value)) {
            let ExpressionKind::Identifier(name, _) = &passed.node else {
                continue;
            };
            let kept_here = consumed.contains_key(name) && !consumed_before.contains(name);
            if kept_here && !seen.insert(name.as_str()) {
                self.errors
                    .push(passed_again_error(name, fn_name, passed.span));
            }
        }
    }

    /// The receiver of `receiver.drop()` when the call names the drop hook of
    /// the receiver's type, rather than an ordinary method that happens to be
    /// called `drop`.
    fn drop_hook_receiver<'e>(
        &self,
        callee: &'e Expression,
        args: &[Expression],
    ) -> Option<&'e Expression> {
        let ExpressionKind::Member(receiver, method) = &callee.node else {
            return None;
        };
        let ExpressionKind::Identifier(method_name, _) = &method.node else {
            return None;
        };
        let receiver_ty = self.types.get(&receiver.id)?;
        (method_name == DROP_HOOK_NAME
            && args.is_empty()
            && runs_drop_hook(&receiver_ty.kind, self.type_definitions))
        .then_some(receiver.as_ref())
    }

    /// `value.drop()` runs the hook and releases the value on the spot, which is
    /// sound only for a value this scope owns: a local it declared, which the
    /// call then consumes, or a call result nothing else holds. Any other
    /// receiver is still held by someone who will release it again.
    fn check_drop_hook_call(
        &mut self,
        receiver: &Expression,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        self.check_expr(receiver, consumed);
        if matches!(receiver.node, ExpressionKind::Call(_, _)) {
            return;
        }
        if let ExpressionKind::Identifier(name, _) = &receiver.node {
            if self.owned_locals.contains(name.as_str()) {
                consumed
                    .entry(name.clone())
                    .or_insert_with(|| ConsumedInfo {
                        by_fn: DROP_HOOK_NAME.to_string(),
                        at_span: receiver.span,
                        chain: vec![],
                    });
                return;
            }
        }
        self.errors.push(TypeError {
            kind: TypeErrorKind::Coded {
                code: DiagnosticCode::OwnDropOfBorrowedValue,
                message: BORROWED_DROP_MESSAGE.to_string(),
                help: Some(BORROWED_DROP_HELP.to_string()),
                expected: None,
                actual: None,
            },
            span: receiver.span,
            source_override: None,
            repair: None,
            related: Vec::new(),
        });
    }

    // TODO: read which parameter an argument binds, and whether it is `out`,
    // from `bind_arguments_to_parameters`, as `body_writes` does. This
    // classifier counts a positional argument after a named one at the next
    // positional slot, where the binding keeps its written position, so the
    // two disagree on such a call.
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
        &mut self,
        args: &[Expression],
        params: &[Parameter],
        summary: &EscapeSummary,
        sink: &Sink,
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
                self.consume_resource_arg(arg, sink, idx, consumed);
            }
        }
    }

    fn consume_resource_args(
        &mut self,
        args: &[Expression],
        params: &[Parameter],
        sink: &Sink,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let mut pos_idx = 0usize;
        for arg in args {
            let (is_out, idx) = Self::arg_classify(arg, params, &mut pos_idx);
            let param_idx = if idx == usize::MAX { 0 } else { idx };
            if !is_out {
                self.consume_resource_arg(arg, sink, param_idx, consumed);
            }
        }
    }

    /// Whether `expr` is a resource, the only kind of value a call consumes:
    /// every other value is lent and stays the caller's.
    fn is_resource_expr(&self, expr: &Expression) -> bool {
        self.types
            .get(&expr.id)
            .is_some_and(|ty| is_resource(&ty.kind, self.type_definitions))
    }

    /// Whether consuming the binding `expr` names hands its reference on: its
    /// type runs a drop hook of its own, rather than reaching the value
    /// through a trait, which shares it.
    fn hands_on_its_reference(&self, expr: &Expression) -> bool {
        self.types
            .get(&expr.id)
            .is_some_and(|ty| hands_on_a_reference(&ty.kind, self.type_definitions))
    }

    /// Marks the variable `arg` names consumed by `sink` when it is a resource,
    /// and records the argument for lowering; any other value is lent and
    /// stays usable.
    ///
    /// `callee_param_idx` is used for chain lookup when `sink` is a known callee name.
    fn consume_resource_arg(
        &mut self,
        arg: &Expression,
        sink: &Sink,
        callee_param_idx: usize,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        let value = argument_value(arg);
        if let Some(captured) = self.lent_closures.remove(&value.id) {
            self.consume_captures(value.id, &captured, consumed);
            return;
        }
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
        if !self.is_resource_expr(ident_expr) {
            return;
        }
        if self.gives_away_a_capture(&name, ident_expr.span) {
            return;
        }
        if self.hands_on_its_reference(ident_expr) {
            self.consumed_arguments.expressions.insert(ident_expr.id);
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

    /// The function a call names: the declaration a free-function callee
    /// resolved to, and otherwise the name it is written as.
    fn named_callee(&self, callee: &Expression, name: &str) -> FunctionId {
        free_callee(callee, name, self.callees)
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

    /// Builds the "consumed because" chain by following `escape_next_hops`
    /// through the escape summaries: one note per step, at the place that
    /// takes it.
    ///
    /// Starts at `sink` / `param_idx` and follows hops until reaching a sink
    /// or a function with no hop data. Empty when no chain info is
    /// available — always so for a dynamic function value, which has no
    /// summary.
    fn build_chain(&self, sink: &Sink, param_idx: usize) -> Vec<RelatedNote> {
        let mut notes = Vec::new();
        let Sink::Function(callee_fn) = sink else {
            return notes;
        };
        let mut current_fn = callee_fn.clone();
        let mut current_param = param_idx;
        let mut visited = std::collections::HashSet::new();
        while visited.insert((current_fn.clone(), current_param)) {
            let Some(site) = (self.escape_summaries.get(&current_fn))
                .and_then(|summary| summary.escape_next_hops.get(&current_param))
            else {
                break;
            };
            let message = format!(
                "consumed because {} \u{2192} {}",
                current_fn,
                step(&site.hop)
            );
            notes.push(RelatedNote {
                message,
                span: site.span,
            });
            let EscapeNextHop::Call { callee, param_slot } = &site.hop else {
                break;
            };
            current_fn = callee.clone();
            current_param = *param_slot;
        }
        notes
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

    /// Returns the summary key of a method-call callee,
    /// resolved to the **defining** class — walks the `base_class` chain so an
    /// inherited method's chain key points at the class that actually has the
    /// escape summary (as [`method_summary`] does).
    ///
    /// Used so `build_chain` can look up the escape summary by its canonical key
    /// rather than the bare method name, and so the user-visible "consumed by"
    /// label names the class that owns the field-store sink rather than the
    /// receiver's static type. Returns `None` for non-method callees or when
    /// the receiver type is unresolved.
    fn extract_method_chain_key(&self, callee: &Expression) -> Option<FunctionId> {
        let (type_name, method_name) = self.method_call_target(callee)?;
        let has_summary = |id: &FunctionId| self.escape_summaries.contains_key(id);
        let owner = method_owner(type_name, method_name, &has_summary, self.type_definitions);
        Some(owner.unwrap_or_else(|| FunctionId::method(type_name, method_name)))
    }

    /// If `callee` is a method call (`Member(receiver, method_name)`), the
    /// escape summary the call reaches ([`method_summary`]).
    ///
    /// Returns `None` when the callee is a free function, the receiver type is
    /// unresolved, or no summary is present for the method.
    fn extract_method_summary(&self, callee: &Expression) -> Option<EscapeSummary> {
        let (type_name, method_name) = self.method_call_target(callee)?;
        method_summary(
            type_name,
            method_name,
            self.escape_summaries,
            self.type_definitions,
        )
    }

    /// The receiver's declared type and the method's name of a method-call
    /// callee, when the receiver is of a named type.
    fn method_call_target<'e>(&'e self, callee: &'e Expression) -> Option<(&'e str, &'e str)> {
        let ExpressionKind::Member(obj_expr, method_expr) = &callee.node else {
            return None;
        };
        let ExpressionKind::Identifier(method_name, _) = &method_expr.node else {
            return None;
        };
        let TypeKind::Custom(type_name, _) = &self.types.get(&obj_expr.id)?.kind else {
            return None;
        };
        Some((type_name, method_name))
    }
}

/// Drop from `state` every name in `names`, bound afresh where they are
/// declared and so never consumed outside it.
fn forget_names(state: &mut Consumed, names: &[String]) {
    for name in names {
        state.remove(name);
    }
}

/// Whether `statement` holds a `return`, which leaves every loop around it.
pub(crate) fn contains_return(statement: &Statement) -> bool {
    match &statement.node {
        StatementKind::Return(_) => true,
        StatementKind::Block(statements) => statements.iter().any(contains_return),
        StatementKind::If(_, then, else_, _) => {
            contains_return(then) || else_.as_deref().is_some_and(contains_return)
        }
        StatementKind::While(_, inner, _) | StatementKind::For(_, _, inner) => {
            contains_return(inner)
        }
        _ => false,
    }
}

/// Every name a `let`/`var` or a loop in `statement` declares, nested blocks
/// included.
fn names_declared_in(statement: &Statement, declared: &mut Vec<String>) {
    match &statement.node {
        StatementKind::Variable(decls, _) => declared.extend(decls.iter().map(|d| d.name.clone())),
        StatementKind::Block(statements) => {
            for inner in statements {
                names_declared_in(inner, declared);
            }
        }
        StatementKind::If(_, then, else_, _) => {
            names_declared_in(then, declared);
            if let Some(else_) = else_ {
                names_declared_in(else_, declared);
            }
        }
        StatementKind::While(_, inner, _) => names_declared_in(inner, declared),
        StatementKind::For(decls, _, inner) => {
            declared.extend(decls.iter().map(|d| d.name.clone()));
            names_declared_in(inner, declared);
        }
        _ => {}
    }
}

/// The value an argument passes: a named argument's value, or the argument.
fn argument_value(arg: &Expression) -> &Expression {
    match &arg.node {
        ExpressionKind::NamedArgument(_, value) => value,
        _ => arg,
    }
}

/// The refusal of a resource handed to one call at a position the call keeps
/// and at another.
fn passed_again_error(name: &str, fn_name: &str, span: Span) -> TypeError {
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnUseOfMovedValue,
            message: format!(
                "'{name}' is passed to '{fn_name}' more than once, and '{fn_name}' keeps it: a resource has one owner\n  fix: pass '{name}' once, and read what the other argument needs from it before the call"
            ),
            help: None,
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related: Vec::new(),
    }
}

/// The refusal of a closure body handing on a resource it captures.
fn given_away_capture_error(name: &str, span: Span) -> TypeError {
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnDropOfBorrowedValue,
            message: format!(
                "'{name}' is captured by the closure, and the closure does not own it: it cannot be kept, moved, returned or dropped here"
            ),
            help: Some(format!(
                "pass '{name}' to the closure as a parameter, or hand it on outside the closure"
            )),
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related: Vec::new(),
    }
}

/// What one step of a "consumed because" chain says the function does.
fn step(hop: &EscapeNextHop) -> String {
    match hop {
        EscapeNextHop::Call { callee, .. } => format!("calls {callee} (passes its argument)"),
        EscapeNextHop::Return => "returns its argument (escape sink)".to_string(),
        EscapeNextHop::ReturnAggregate => {
            "returns its argument in an aggregate (escape sink)".to_string()
        }
        EscapeNextHop::FieldStore { field } => {
            format!("stores its argument into field '{field}' (escape sink)")
        }
        EscapeNextHop::ElementStore => {
            "stores its argument into an element (escape sink)".to_string()
        }
        EscapeNextHop::Built => "builds a value that holds its argument (escape sink)".to_string(),
        EscapeNextHop::ClosureCapture { returned: true } => {
            "captures its argument in a returned closure (escape sink)".to_string()
        }
        EscapeNextHop::ClosureCapture { returned: false } => {
            "captures its argument in a closure (escape sink)".to_string()
        }
        EscapeNextHop::DynamicFn { fn_name } => {
            format!("passes its argument to dynamic fn '{fn_name}' (escape sink)")
        }
    }
}
