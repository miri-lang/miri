// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which of a class's own method bodies are correct at every instantiation of
//! the class, because the runtime settles everything in them that depends on a
//! type argument. This is the one statement of the rule; the dispatch that
//! shares a built-in collection's body and the MIR verifier's exemption both
//! read the set it records, [`ClassDefinition::runtime_settled_methods`].
//!
//! A body the class declares is settled when both hold:
//!
//! - **Every call is settled.** It calls a `runtime` function, a built-in
//!   variant constructor (`Some`, `Ok`, `Err`), `self.m()` for a method `m` of
//!   this class that is itself settled, or `super.m()` for a settled method of
//!   the class it extends. A call to a function value, a constructor, a method
//!   the class only inherits from a trait, or anything else is not.
//! - **Nothing reads a type-parameter value except to hand it on.** No
//!   operator, conversion, interpolation, collection literal or store has an
//!   operand whose type mentions one of the class's type parameters; no `for`
//!   iterates such a value, no `match` on one tests a literal or binds, and no
//!   index reads one out of anything but `self`. Each of those computes at the
//!   pointer-width stand-in for the type argument — `==` compares a string's
//!   address, a store takes no reference to what it stores. An expression the
//!   checker recorded no type for counts as reading one.
//!
//! What stays is a body that moves elements between the caller and the
//! runtime: `pop` reads `self[len - 1]` and hands the list's own reference
//! over with a runtime take, and only a body that names the intrinsic can pair
//! with it.
//!
//! The rule is judged with this class's own bodies as the callees of `self`;
//! a subclass receiver, whose overrides would answer instead, is not covered.
//! The answer for one method depends on the answers for the methods it calls,
//! so the set is a greatest fixpoint: every declared body starts settled, and
//! a body leaves once it calls one that has left.
//!
//! [`ClassDefinition::runtime_settled_methods`]:
//! crate::type_checker::context::ClassDefinition::runtime_settled_methods

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::pattern::{MatchBranch, Pattern};
use crate::ast::statement::VariableDeclaration;
use crate::ast::types::{Type, TypeKind};
use crate::ast::{Statement, StatementKind};
use crate::type_checker::context::{ClassDefinition, GenericDefinition, TypeDefinition};
use crate::type_checker::function_analysis::CalleeKind;
use crate::type_checker::generics::is_generic_parameter_kind;
use crate::type_checker::statements::declarations::class_def::method_body;
use crate::type_checker::TypeChecker;
use std::collections::{BTreeSet, HashMap};

impl TypeChecker {
    /// Records on the class `class_name` which of the bodies declared in
    /// `method_statements` are settled by the runtime.
    ///
    /// Runs once those bodies are checked, since it reads the callee and the
    /// type the checker recorded for each of their expressions.
    pub(crate) fn record_runtime_settled_methods(
        &mut self,
        class_name: &str,
        method_statements: &[&Statement],
    ) {
        let settled = self.runtime_settled_methods(class_name, method_statements);
        if let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get_mut(class_name)
        {
            class_def.runtime_settled_methods = settled;
        }
    }

    fn runtime_settled_methods(
        &self,
        class_name: &str,
        method_statements: &[&Statement],
    ) -> BTreeSet<String> {
        let definitions = &self.type_table.global_type_definitions;
        let Some(TypeDefinition::Class(class_def)) = definitions.get(class_name) else {
            return BTreeSet::new();
        };
        let bodies: Vec<(&str, &Statement)> = method_statements
            .iter()
            .filter_map(|stmt| match &stmt.node {
                StatementKind::FunctionDeclaration(decl) => {
                    method_body(decl).map(|body| (decl.name.as_str(), body))
                }
                _ => None,
            })
            .collect();
        let inherited = base_class_settled_methods(class_def, definitions);
        let mut settled: BTreeSet<&str> = bodies.iter().map(|(name, _)| *name).collect();
        loop {
            let audit = SettlementAudit {
                types: &self.type_table.types,
                callee_kinds: &self.fn_analysis.callee_kinds,
                class_generics: class_def.generics.as_ref(),
                settled: &settled,
                inherited,
            };
            let kept: BTreeSet<&str> = bodies
                .iter()
                .filter(|(name, body)| settled.contains(name) && audit.statement(body))
                .map(|(name, _)| *name)
                .collect();
            if kept.len() == settled.len() {
                return kept.into_iter().map(str::to_string).collect();
            }
            settled = kept;
        }
    }
}

/// The settled methods of the class `class_def` extends, which a `super` call
/// reaches; none for a class that extends nothing.
fn base_class_settled_methods<'d>(
    class_def: &ClassDefinition,
    definitions: &'d HashMap<String, TypeDefinition>,
) -> Option<&'d BTreeSet<String>> {
    match definitions.get(class_def.base_class.as_deref()?)? {
        TypeDefinition::Class(base_def) => Some(&base_def.runtime_settled_methods),
        TypeDefinition::Struct(_)
        | TypeDefinition::Enum(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_)
        | TypeDefinition::Trait(_) => None,
    }
}

/// One pass over method bodies against the current guess at the settled set.
struct SettlementAudit<'a> {
    /// The type the checker recorded for each expression, by id.
    types: &'a HashMap<usize, Type>,
    /// How each identifier the checker resolved is called, by id.
    callee_kinds: &'a HashMap<usize, CalleeKind>,
    class_generics: Option<&'a Vec<GenericDefinition>>,
    /// The class's own methods still believed settled.
    settled: &'a BTreeSet<&'a str>,
    /// The settled methods of the class this one extends.
    inherited: Option<&'a BTreeSet<String>>,
}

impl SettlementAudit<'_> {
    fn statement(&self, stmt: &Statement) -> bool {
        match &stmt.node {
            StatementKind::Expression(expr) => self.expression(expr),
            StatementKind::Block(stmts) => stmts.iter().all(|s| self.statement(s)),
            StatementKind::Variable(decls, _) => self.declarations(decls),
            StatementKind::If(cond, then_branch, else_branch, _) => {
                self.expression(cond)
                    && self.statement(then_branch)
                    && else_branch.as_deref().is_none_or(|s| self.statement(s))
            }
            StatementKind::While(cond, body, _) => self.expression(cond) && self.statement(body),
            // A loop over a parameter value reads its elements through calls
            // the body never spells and binds each one.
            StatementKind::For(decls, iterable, body)
            | StatementKind::GpuFrame(decls, iterable, body)
            | StatementKind::Forall {
                vars: decls,
                iterable,
                body,
                ..
            } => self.operands(&[iterable]) && self.declarations(decls) && self.statement(body),
            StatementKind::GpuFrameBlock(body) => self.statement(body),
            StatementKind::Return(value) => value.as_deref().is_none_or(|e| self.expression(e)),
            // Nothing here runs when the method does: a nested declaration only
            // runs if called, and a call to it is judged where it is made.
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Use(..)
            | StatementKind::Type(..)
            | StatementKind::FunctionDeclaration(_)
            | StatementKind::Enum(..)
            | StatementKind::Struct(..)
            | StatementKind::Class(_)
            | StatementKind::Trait(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..) => true,
        }
    }

    fn declarations(&self, decls: &[VariableDeclaration]) -> bool {
        decls
            .iter()
            .filter_map(|decl| decl.initializer.as_deref())
            .all(|init| self.expression(init))
    }

    fn expression(&self, expr: &Expression) -> bool {
        match &expr.node {
            ExpressionKind::Call(callee, args) => self.callee(callee) && self.all(args),
            ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
                self.operands(&[lhs, rhs])
            }
            ExpressionKind::Unary(_, operand) => self.operands(&[operand]),
            ExpressionKind::Cast(value, _) => self.operands(&[value]),
            ExpressionKind::FormattedString(parts) => {
                parts.iter().all(|part| self.operands(&[part]))
            }
            ExpressionKind::List(elements)
            | ExpressionKind::Tuple(elements)
            | ExpressionKind::Set(elements)
            | ExpressionKind::Array(elements, _) => {
                !self.reads_parameter(expr) && self.all(elements)
            }
            ExpressionKind::Map(entries) => !self.reads_parameter(expr) && self.entries(entries),
            // A store, plain or compound, of a parameter value: the shared body
            // would store it as a word it takes no reference to, and release
            // none it overwrites. The checker records the stored value's type,
            // not the target's; the two agree, so the value decides.
            ExpressionKind::Assignment(target, _, value) => {
                self.operands(&[value]) && self.expression(left_hand_side(target))
            }
            ExpressionKind::Index(base, index) => self.index(base, index),
            ExpressionKind::Member(object, _) => self.expression(object),
            ExpressionKind::Guard(_, inner) | ExpressionKind::NamedArgument(_, inner) => {
                self.expression(inner)
            }
            ExpressionKind::EnumValue(_, args) => self.all(args),
            ExpressionKind::Conditional(cond, then_value, else_value, _) => {
                self.expression(cond)
                    && self.expression(then_value)
                    && else_value.as_deref().is_none_or(|e| self.expression(e))
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start) && end.as_deref().is_none_or(|e| self.expression(e))
            }
            ExpressionKind::Match(scrutinee, branches) => self.match_(scrutinee, branches),
            ExpressionKind::Block(stmts, value) => {
                stmts.iter().all(|s| self.statement(s)) && self.expression(value)
            }
            ExpressionKind::Lambda(lambda) => self.statement(&lambda.body),
            ExpressionKind::Literal(_)
            | ExpressionKind::Identifier(..)
            | ExpressionKind::Super
            | ExpressionKind::Type(..)
            | ExpressionKind::GenericType(..)
            | ExpressionKind::TypeDeclaration(..)
            | ExpressionKind::ImportPath(..)
            | ExpressionKind::StructMember(..) => true,
        }
    }

    fn all(&self, exprs: &[Expression]) -> bool {
        exprs.iter().all(|e| self.expression(e))
    }

    fn entries(&self, entries: &[(Expression, Expression)]) -> bool {
        entries
            .iter()
            .all(|(key, value)| self.expression(key) && self.expression(value))
    }

    /// Reading `self[i]` hands an element on; indexing any other value of a
    /// parameter type reads one out at the stand-in width.
    fn index(&self, base: &Expression, index: &Expression) -> bool {
        (is_self(base) || !self.reads_parameter(base))
            && self.expression(base)
            && self.expression(index)
    }

    /// A `match` on a parameter value that tests a literal or binds compares
    /// or copies the value at the stand-in width.
    fn match_(&self, scrutinee: &Expression, branches: &[MatchBranch]) -> bool {
        let inspects = branches
            .iter()
            .flat_map(|branch| &branch.patterns)
            .any(pattern_inspects_value);
        (!inspects || !self.reads_parameter(scrutinee))
            && self.expression(scrutinee)
            && branches.iter().all(|branch| {
                branch.guard.as_deref().is_none_or(|g| self.expression(g))
                    && self.statement(&branch.body)
            })
    }

    /// Whether an operation applied to `operands` is settled: none of them
    /// reads a type-parameter value, and each is settled itself.
    fn operands(&self, operands: &[&Expression]) -> bool {
        operands
            .iter()
            .all(|operand| !self.reads_parameter(operand) && self.expression(operand))
    }

    /// Whether the call through `callee` is settled, judged by what the name
    /// resolved to where it was written.
    fn callee(&self, callee: &Expression) -> bool {
        match &callee.node {
            ExpressionKind::Identifier(_, None) => matches!(
                self.callee_kinds.get(&callee.id),
                Some(CalleeKind::Runtime | CalleeKind::VariantConstructor)
            ),
            ExpressionKind::Member(receiver, method) => {
                let ExpressionKind::Identifier(method_name, _) = &method.node else {
                    return false;
                };
                if is_self(receiver) {
                    self.settled.contains(method_name.as_str())
                } else if matches!(receiver.node, ExpressionKind::Super) {
                    self.inherited.is_some_and(|set| set.contains(method_name))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Whether the value of `expr` has a type that mentions one of the class's
    /// type parameters. An expression with no recorded type counts as one: an
    /// unknown operand is not known to be settled.
    fn reads_parameter(&self, expr: &Expression) -> bool {
        self.types
            .get(&expr.id)
            .is_none_or(|ty| self.mentions_parameter(ty))
    }

    fn mentions_parameter(&self, ty: &Type) -> bool {
        let in_expr = |e: &Expression| matches!(&e.node, ExpressionKind::Type(inner, _) if self.mentions_parameter(inner));
        match &ty.kind {
            TypeKind::Custom(_, None) | TypeKind::Generic(..) => {
                is_generic_parameter_kind(&ty.kind, self.class_generics)
            }
            TypeKind::List(e) | TypeKind::Set(e) | TypeKind::Future(e) => in_expr(e),
            TypeKind::Array(e, _) => in_expr(e),
            TypeKind::Map(k, v) | TypeKind::Result(k, v) => in_expr(k) || in_expr(v),
            TypeKind::Tuple(elems) => elems.iter().any(in_expr),
            TypeKind::Custom(_, Some(args)) => args.iter().any(in_expr),
            TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
                self.mentions_parameter(inner)
            }
            TypeKind::Function(function) => {
                function.params.iter().any(|param| in_expr(&param.typ))
                    || function.return_type.as_deref().is_some_and(in_expr)
            }
            TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Void
            | TypeKind::Error => false,
        }
    }
}

/// Whether `expr` is the method's own receiver, `self`.
fn is_self(expr: &Expression) -> bool {
    matches!(&expr.node, ExpressionKind::Identifier(name, None) if name == "self")
}

/// Whether a branch pattern looks at the scrutinee's value — compares it with
/// a literal or binds it — rather than only at which variant it is.
fn pattern_inspects_value(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Literal(_) | Pattern::Regex(_) | Pattern::Identifier(_) => true,
        Pattern::Tuple(parts) => parts.iter().any(pattern_inspects_value),
        Pattern::EnumVariant(_, bindings) => bindings.iter().any(pattern_inspects_value),
        Pattern::Default | Pattern::Member(..) => false,
    }
}

/// The expression an assignment writes to.
fn left_hand_side(target: &LeftHandSideExpression) -> &Expression {
    match target {
        LeftHandSideExpression::Identifier(e)
        | LeftHandSideExpression::Member(e)
        | LeftHandSideExpression::Index(e) => e,
    }
}
