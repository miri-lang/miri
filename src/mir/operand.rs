// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::literal::Literal;
use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::body::Body;
use crate::mir::place::{Place, PlaceElem};
use crate::mir::types::MirType;
use std::fmt;

/// An operand for an Rvalue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Operand {
    /// Moves the value out of the place.
    ///
    /// Reference counting does not honour this: the source local is still
    /// released when its scope ends, so a move of a managed place hands the
    /// value on without retaining it and leaves one allocation carrying two
    /// releases. Consumers that release what they receive must read the place
    /// by [`Operand::Copy`] instead — several lowering seams normalize to
    /// `Copy` for exactly this reason.
    Move(Place),
    /// Copies the value from the place.
    Copy(Place),
    /// A constant value.
    Constant(Box<Constant>),
}

impl Operand {
    /// Returns a reference to the type of this operand.
    ///
    /// For place operands (Move/Copy), returns the type from the body's local declarations.
    /// For constants, returns the constant's type.
    ///
    /// **Note**: This method ignores place projections (e.g., `place[i].0` returns the
    /// base type, not the element or field type). Use `ty_projected` for
    /// projection-aware type resolution.
    pub fn ty<'a>(&'a self, body: &'a Body) -> &'a Type {
        match self {
            Operand::Move(place) | Operand::Copy(place) => &body.local_decls[place.local.0].ty,
            Operand::Constant(c) => &c.ty,
        }
    }

    /// Returns the projected MIR type of this operand, if resolvable.
    ///
    /// For place operands (Move/Copy), walks the projection chain to resolve
    /// the final MIR type. For constants, returns the constant's MIR type.
    ///
    /// Handles the following projection elements:
    /// - `Index` on `Array<T,N>`, `List<T>`, `Set<T>`, `Map<K,V>` → yields element/value type
    /// - `Field(i)` on `Option<T>` → yields inner type `T` (only for field 0)
    /// - `Field(i)` on `Tuple(T0, T1, ...)` → yields `Ti`
    /// - `Field(i)` on custom `Struct`/`Class` types → looks up in `body.field_types`
    /// - `Field(i)` on `Function` (closure) → looks up in `body.closure_capture_types`
    /// - `Deref` → unresolvable (returns `None`)
    /// - Enum `Field(i)` → unresolvable (variant not known at compile time)
    ///
    /// Returns `None` if:
    /// - A projection cannot be resolved (unresolvable enum variant, missing field, etc.)
    /// - A `Deref` is encountered (pointer target type not accessible)
    /// - An `Index` is on a non-collection type
    ///
    /// **Invariant**: The result (when `Some`) always represents the most-projected MIR type,
    /// not the base type. This is suitable for width checks and managed-type queries in codegen and optimization.
    pub fn ty_projected(&self, body: &Body) -> Option<MirType> {
        match self {
            Operand::Constant(c) => Some(MirType::from_type_kind(&c.ty.kind)),
            Operand::Move(place) | Operand::Copy(place) => resolve_place_mir_type(place, body),
        }
    }
}

/// Resolves the MIR type of a place through its projections.
///
/// Mirrors the logic in `src/mir/optimization/perceus.rs::is_place_managed` to resolve
/// types through `Field` and `Index` projections. Exhaustively handles all
/// `PlaceElem` variants.
///
/// Returns `None` if any projection cannot be resolved (e.g., enum field, deref, or
/// index on a non-collection). The caller can use the returned `MirType` to check
/// width, layout, or managed-type properties.
fn resolve_place_mir_type(place: &Place, body: &Body) -> Option<MirType> {
    // Start from the MIR-level type of the base local.
    let mut current: MirType = body.local_decls[place.local.0].mir_ty.clone();

    for elem in &place.projection {
        match elem {
            PlaceElem::Deref => {
                // Pointer target type is not accessible; cannot resolve further.
                return None;
            }
            PlaceElem::Index(_) => {
                // Extract element type from collection.
                current = match current {
                    MirType::Array(elem) | MirType::List(elem) | MirType::Set(elem) => *elem,
                    MirType::Map(_, v) => *v,
                    _ => return None,
                };
            }
            PlaceElem::Field(i) => {
                let next_mir = match &current {
                    // Option<T>.Field(0) → T
                    MirType::Option(inner) if *i == 0 => *inner.clone(),
                    // Tuple(T0, T1, ...).Field(i) → Ti
                    MirType::Tuple(elems) => elems.get(*i)?.clone(),
                    // Custom struct/class → look up in field_types and convert to MirType
                    MirType::Custom(name) => {
                        let field_ty = body.field_types.get(name.as_str())?.get(*i)?;
                        MirType::from_type_kind(&field_ty.kind)
                    }
                    // Closure local → look up in closure_capture_types and convert to MirType
                    MirType::Function => {
                        let captures = body.closure_capture_types.get(&place.local)?;
                        let capture_ty = captures.get(*i)?;
                        MirType::from_type_kind(&capture_ty.kind)
                    }
                    _ => return None,
                };
                current = next_mir;
            }
        }
    }

    Some(current)
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Move(place) => write!(f, "move {}", place),
            Operand::Copy(place) => write!(f, "{}", place), // Implicit copy usually
            Operand::Constant(c) => write!(f, "const {}", c),
        }
    }
}

/// A constant value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Constant {
    pub span: Span,
    pub ty: Type,
    pub literal: Literal,
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.literal)
    }
}
