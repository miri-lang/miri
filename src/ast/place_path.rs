// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The shape of a place written as fields and elements of a root.
//!
//! `grid[i].cells[j]`, `self.slot` and `h.conn` are places: a root expression
//! and a chain of field and element steps taken from it. Lowering a write,
//! finding the parameters and receivers a body writes, refusing a store
//! through an immutable binding and refusing a store that closes a cycle all
//! read a place's shape, so they read it here, and a new kind of step is
//! taught once.

use crate::ast::expression::{Expression, ExpressionKind};

/// One field or element step of a place, read off the expression that takes
/// it.
pub enum PathLink<'e> {
    /// `object.prop`: the object, the field, and the whole expression.
    Member(&'e Expression, &'e Expression, &'e Expression),
    /// `collection[index]`: the collection, the index, and the whole
    /// expression.
    Index(&'e Expression, &'e Expression, &'e Expression),
}

impl<'e> PathLink<'e> {
    /// The value the step is taken from: `object` in `object.prop`,
    /// `collection` in `collection[index]`.
    pub fn holder(&self) -> &'e Expression {
        match self {
            PathLink::Member(holder, _, _) | PathLink::Index(holder, _, _) => holder,
        }
    }

    /// The place the step reaches: the whole `object.prop` or
    /// `collection[index]`.
    pub fn place(&self) -> &'e Expression {
        match self {
            PathLink::Member(_, _, place) | PathLink::Index(_, _, place) => place,
        }
    }
}

/// The expression a place starts at, and the field and element steps taken
/// from it, in the order they are taken: the step nearest the root first.
pub fn path_links(expr: &Expression) -> (&Expression, Vec<PathLink<'_>>) {
    let mut links = Vec::new();
    let mut root = expr;
    loop {
        if let ExpressionKind::Member(inner, prop) = &root.node {
            links.push(PathLink::Member(inner, prop, root));
            root = inner;
        } else if let ExpressionKind::Index(collection, index) = &root.node {
            links.push(PathLink::Index(collection, index, root));
            root = collection;
        } else {
            break;
        }
    }
    links.reverse();
    (root, links)
}

/// The expression a place starts at: `expr` with every field and element step
/// taken off.
pub fn path_root(expr: &Expression) -> &Expression {
    let mut root = expr;
    while let ExpressionKind::Member(inner, _) | ExpressionKind::Index(inner, _) = &root.node {
        root = inner;
    }
    root
}

/// The name of the binding a place starts at: `buf` for `buf`, `buf[i]` and
/// `buf[i].x`; `None` for a place starting at anything but a name.
pub fn path_root_name(expr: &Expression) -> Option<&str> {
    let ExpressionKind::Identifier(name, _) = &path_root(expr).node else {
        return None;
    };
    Some(name)
}
