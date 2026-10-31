// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::fmt;

/// A local variable in the MIR.
/// This can be a user-defined variable or a temporary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Local(pub usize);

impl fmt::Display for Local {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

/// A place in memory (e.g., a variable, a field, an array element).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub local: Local,
    pub projection: Vec<PlaceElem>,
}

impl Place {
    pub fn new(local: Local) -> Self {
        Self {
            local,
            projection: Vec::new(),
        }
    }

    /// Every local this place indexes by, outermost first.
    pub fn index_locals(&self) -> impl Iterator<Item = Local> + '_ {
        self.projection.iter().filter_map(|elem| match elem {
            PlaceElem::Index(local, _) => Some(*local),
            PlaceElem::Deref | PlaceElem::Field(_) => None,
        })
    }

    /// Whether a read or write of `self` may reach the value at `other`: both
    /// start at one local and `self`'s projection is a prefix of `other`'s in
    /// which an index matches any index — two index locals may hold one value —
    /// and a dereference matches anything.
    pub fn covers(&self, other: &Place) -> bool {
        self.local == other.local
            && self.projection.len() <= other.projection.len()
            && self
                .projection
                .iter()
                .zip(&other.projection)
                .all(|pair| match pair {
                    (PlaceElem::Field(a), PlaceElem::Field(b)) => a == b,
                    (PlaceElem::Index(_, _), PlaceElem::Index(_, _))
                    | (
                        PlaceElem::Deref,
                        PlaceElem::Deref | PlaceElem::Field(_) | PlaceElem::Index(_, _),
                    )
                    | (PlaceElem::Field(_) | PlaceElem::Index(_, _), PlaceElem::Deref) => true,
                    (PlaceElem::Field(_), PlaceElem::Index(_, _))
                    | (PlaceElem::Index(_, _), PlaceElem::Field(_)) => false,
                })
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.local)?;
        for elem in &self.projection {
            match elem {
                PlaceElem::Deref => write!(f, ".*")?,
                PlaceElem::Field(idx) => write!(f, ".{}", idx)?,
                PlaceElem::Index(local, Bounds::Checked) => write!(f, "[{}]", local)?,
                PlaceElem::Index(local, Bounds::Proven) => write!(f, "[{} in bounds]", local)?,
            }
        }
        Ok(())
    }
}

/// A projection element that modifies a place.
///
/// Projections allow accessing parts of a composite value:
/// fields of structs/tuples, elements of arrays, or dereferencing pointers.
///
/// Two elements are equal when they name the same part of a value: an
/// index's [`Bounds`] says how the access is made, not what it reaches, so
/// it takes no part in equality or hashing.
#[derive(Debug, Clone)]
pub enum PlaceElem {
    /// Dereference a pointer/reference: `*place`
    Deref,
    /// Access a field by index: `place.0`, `place.field`
    Field(usize),
    /// Index into an array/list: `place[index]`
    Index(Local, Bounds),
}

impl PlaceElem {
    /// An index access by `local` that compares it to the length: every
    /// index starts checked until a pass proves it in bounds.
    pub fn index(local: Local) -> Self {
        PlaceElem::Index(local, Bounds::Checked)
    }
}

impl PartialEq for PlaceElem {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PlaceElem::Deref, PlaceElem::Deref) => true,
            (PlaceElem::Field(a), PlaceElem::Field(b)) => a == b,
            (PlaceElem::Index(a, _), PlaceElem::Index(b, _)) => a == b,
            (PlaceElem::Deref | PlaceElem::Field(_) | PlaceElem::Index(_, _), _) => false,
        }
    }
}

impl Eq for PlaceElem {}

impl std::hash::Hash for PlaceElem {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            PlaceElem::Deref => {}
            PlaceElem::Field(index) => index.hash(state),
            PlaceElem::Index(local, _) => local.hash(state),
        }
    }
}

/// Whether an index access compares its index to the collection's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bounds {
    /// Code generation compares the index to the length and traps when it
    /// is out of range.
    Checked,
    /// A loop guard proves the index non-negative and below the length, so
    /// the access is emitted without the comparison. Set by
    /// `crate::mir::optimization::in_bounds` and re-proven by
    /// `crate::mir::verify::verify_in_bounds` on the bodies code generation
    /// receives.
    Proven,
}

/// The context in which a place is used.
///
/// This is used by the visitor pattern to distinguish between
/// different kinds of place accesses for analysis passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceContext {
    /// The place is read but not modified (e.g., `Copy`, operand use)
    NonMutatingUse,
    /// The place is written to (e.g., assignment destination)
    MutatingUse,
    /// Storage for the local begins (variable comes into scope)
    StorageLive,
    /// Storage for the local ends (variable goes out of scope)
    StorageDead,
}
