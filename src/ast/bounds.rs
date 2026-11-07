// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bounds a type parameter declares, and the relations a `type`
//! statement declares: a list of keyword-and-type pairs.
//!
//! `<T implements Comparable and Hashable>` bounds `T` by two bounds, each an
//! `implements` naming one trait; `<T>` by none; `<T is Real>` by one `is`
//! naming a type set. The list is kept in the order it is written. Every
//! question about a parameter's bounds — does an argument meet them, do they
//! entail another bound, which of them declares a member — is asked over the
//! whole list, so no reader can take a list of several for a single bound or
//! for none.

use std::fmt;

use crate::ast::types::{Type, TypeDeclarationKind};

/// One bound: the keyword it is written with and the type it names
/// (`implements Comparable`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bound {
    pub kind: TypeDeclarationKind,
    pub ty: Type,
}

impl Bound {
    pub fn new(kind: TypeDeclarationKind, ty: Type) -> Self {
        Self { kind, ty }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind, self.ty)
    }
}

/// The bounds one type parameter declares, in the order they are written:
/// none, one, or several joined with `and`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Bounds(Vec<Bound>);

impl Bounds {
    /// No bound: the parameter stands for any type.
    pub fn none() -> Self {
        Self(Vec::new())
    }

    /// The single bound `kind ty`.
    pub fn one(kind: TypeDeclarationKind, ty: Type) -> Self {
        Self(vec![Bound::new(kind, ty)])
    }

    pub fn new(bounds: Vec<Bound>) -> Self {
        Self(bounds)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Bound> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The bound, when exactly one is declared.
    pub fn only(&self) -> Option<&Bound> {
        match self.0.as_slice() {
            [bound] => Some(bound),
            _ => None,
        }
    }

    /// The same bounds with each type rewritten by `rewrite`.
    pub fn map_types(&self, mut rewrite: impl FnMut(&Type) -> Type) -> Self {
        Self(
            self.0
                .iter()
                .map(|bound| Bound::new(bound.kind, rewrite(&bound.ty)))
                .collect(),
        )
    }

    /// Whether a type meets these bounds: it meets every one, as `meets`
    /// judges each.
    pub fn met_by<'b>(&'b self, meets: impl FnMut(&'b Bound) -> bool) -> bool {
        self.missed_by(meets).is_none()
    }

    /// The first bound a type misses, as `meets` judges each.
    pub fn missed_by<'b>(&'b self, mut meets: impl FnMut(&'b Bound) -> bool) -> Option<&'b Bound> {
        self.0.iter().find(|&bound| !meets(bound))
    }

    /// Whether these bounds, a parameter's own, entail a requirement: one
    /// of them does, as `entails` judges each. A parameter with no bound
    /// entails none.
    pub fn entail<'b>(&'b self, entails: impl FnMut(&'b Bound) -> bool) -> bool {
        self.0.iter().any(entails)
    }

    /// The first bound whose type declares a member, as `declares` judges
    /// each type.
    pub fn declaring<'b>(
        &'b self,
        mut declares: impl FnMut(&'b Type) -> bool,
    ) -> Option<&'b Bound> {
        self.0.iter().find(|&bound| declares(&bound.ty))
    }

    /// The types the parameter extends: its `extends` bounds.
    pub fn extended(&self) -> impl Iterator<Item = &Type> {
        self.0
            .iter()
            .filter(|bound| bound.kind == TypeDeclarationKind::Extends)
            .map(|bound| &bound.ty)
    }
}

impl<'b> IntoIterator for &'b Bounds {
    type Item = &'b Bound;
    type IntoIter = std::slice::Iter<'b, Bound>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// `implements Comparable and Hashable`: the bounds as they are written, the
/// keyword once for a run of bounds that share it.
impl fmt::Display for Bounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut previous: Option<TypeDeclarationKind> = None;
        for (index, bound) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(" and ")?;
            }
            if previous != Some(bound.kind) {
                write!(f, "{} ", bound.kind)?;
            }
            write!(f, "{}", bound.ty)?;
            previous = Some(bound.kind);
        }
        Ok(())
    }
}
