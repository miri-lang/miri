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
            PlaceElem::Index(local) => Some(*local),
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
                    (PlaceElem::Index(_), PlaceElem::Index(_))
                    | (
                        PlaceElem::Deref,
                        PlaceElem::Deref | PlaceElem::Field(_) | PlaceElem::Index(_),
                    )
                    | (PlaceElem::Field(_) | PlaceElem::Index(_), PlaceElem::Deref) => true,
                    (PlaceElem::Field(_), PlaceElem::Index(_))
                    | (PlaceElem::Index(_), PlaceElem::Field(_)) => false,
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
                PlaceElem::Index(local) => write!(f, "[{}]", local)?,
            }
        }
        Ok(())
    }
}

/// A projection element that modifies a place.
///
/// Projections allow accessing parts of a composite value:
/// fields of structs/tuples, elements of arrays, or dereferencing pointers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlaceElem {
    /// Dereference a pointer/reference: `*place`
    Deref,
    /// Access a field by index: `place.0`, `place.field`
    Field(usize),
    /// Index into an array/list: `place[index]`
    Index(Local),
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
