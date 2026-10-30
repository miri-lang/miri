// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Builders for hand-built bodies over values held as their bytes.
//!
//! A tuple of scalars is held as its bytes by every program, so these bodies
//! need no type table: the classifier is set over an empty one.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::{
    ArgMode, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::ModuleId;
use std::collections::HashMap;
use std::rc::Rc;

pub fn span() -> Span {
    Span::new(0, 0)
}

pub fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

/// The type expression naming `kind`.
pub fn type_expr(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
        span: span(),
    }
}

/// A value held as its bytes: two scalars.
pub fn pair() -> TypeKind {
    TypeKind::Tuple(vec![type_expr(TypeKind::Int), type_expr(TypeKind::Int)])
}

/// A value holding a [`pair`] and an `int`.
pub fn holder() -> TypeKind {
    TypeKind::Tuple(vec![type_expr(pair()), type_expr(TypeKind::Int)])
}

/// A counted tuple holding a [`pair`] beside a string.
pub fn counted_holder() -> TypeKind {
    TypeKind::Tuple(vec![type_expr(pair()), type_expr(TypeKind::String)])
}

/// A value holding two [`pair`]s, which a runtime index can pick between.
pub fn twins() -> TypeKind {
    TypeKind::Tuple(vec![type_expr(pair()), type_expr(pair())])
}

/// A value holding a [`holder`] and an `int`.
pub fn nest() -> TypeKind {
    TypeKind::Tuple(vec![type_expr(holder()), type_expr(TypeKind::Int)])
}

pub fn place(local: usize) -> Place {
    Place::new(Local(local))
}

pub fn field(local: usize, index: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Field(index)],
    }
}

/// `_local` reached through `projection`.
pub fn projected(local: usize, projection: Vec<PlaceElem>) -> Place {
    Place {
        local: Local(local),
        projection,
    }
}

pub fn copy(local: usize) -> Operand {
    Operand::Copy(place(local))
}

pub fn assign(dest: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(dest, rvalue),
        span: span(),
    }
}

pub fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

pub fn goto_exit() -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(1),
    }
}

/// A call to the Miri function `callee`, going on to block 1.
pub fn call(args: Vec<Operand>, arg_modes: Vec<ArgMode>, destination: Place) -> TerminatorKind {
    TerminatorKind::Call {
        func: Operand::function(
            Symbol::declared_function(&ModuleId::Program, "callee"),
            span(),
        ),
        args,
        arg_modes,
        arg_handles: Vec::new(),
        destination,
        target: Some(BasicBlock(1)),
    }
}

/// A body over locals of `kinds`, `_0` first, taking `arg_count` of them, none
/// `out`, that runs `entry` and then returns.
pub fn body_over(kinds: &[TypeKind], arg_count: usize, entry: BasicBlockData) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.out_params = vec![false; arg_count];
    body.basic_blocks.push(entry);
    body.basic_blocks
        .push(block(Vec::new(), TerminatorKind::Return));
    body.rc_classifier = RcClassifier::new(Rc::new(HashMap::new()));
    body
}

/// The locals every fixture uses: `_0`, `_1` and `_2` are [`pair`]s and `_3`
/// a [`holder`].
pub fn value_locals() -> Vec<TypeKind> {
    vec![pair(), pair(), pair(), holder()]
}

/// A caller over [`value_locals`] whose one block makes `kind`.
pub fn caller(kind: TerminatorKind) -> Body {
    body_over(&value_locals(), 0, block(Vec::new(), kind))
}

/// The locals the projection fixtures use: `_0` to `_2` are [`pair`]s, `_3` a
/// [`holder`], `_4` a [`counted_holder`], `_5` [`twins`], `_6` an `int` and
/// `_7` a [`nest`].
pub fn holder_locals() -> Vec<TypeKind> {
    vec![
        pair(),
        pair(),
        pair(),
        holder(),
        counted_holder(),
        twins(),
        TypeKind::Int,
        nest(),
    ]
}

/// A caller over [`holder_locals`] whose one block makes `kind`.
pub fn holder_caller(kind: TerminatorKind) -> Body {
    body_over(&holder_locals(), 0, block(Vec::new(), kind))
}
