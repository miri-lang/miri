// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::body::Body;
use crate::mir::place::{Local, Place, PlaceElem};
use crate::mir::symbol::{RuntimeCallee, Symbol};
use crate::mir::types::MirType;
use crate::runtime_effects::RuntimeEffect;
use std::fmt;

/// An operand for an Rvalue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Operand {
    /// Hands the reference the place holds on to the consumer.
    ///
    /// A move is a transfer: whatever writes one deletes or re-binds the
    /// source's own release in the same rewrite, so the reference is released
    /// exactly once, by its new holder, and is never retained for it. Lowering
    /// spells a move only where it builds such a sequence itself, as the
    /// copy-on-write check does (`tmp = cow(move x); x = move tmp`); a read of
    /// a named local is always a [`Operand::Copy`]. The reference-count passes
    /// turn a copy into a move where the source dies at the read.
    Move(Place),
    /// Reads the value from the place, which keeps its own reference: a
    /// consumer that holds on to a counted value is funded with a retain.
    Copy(Place),
    /// A constant value.
    Constant(Box<Constant>),
    /// A function named by its symbol: a call's callee, or a function's
    /// address taken as a value.
    Function(Box<FunctionConstant>),
}

impl Operand {
    /// The operand naming the function `symbol`, typed as a function name is.
    pub fn function(symbol: Symbol, span: Span) -> Self {
        Operand::Function(Box::new(FunctionConstant {
            span,
            ty: Type::new(TypeKind::Identifier, span),
            symbol,
        }))
    }

    /// The operand naming the function the runtime library exports as
    /// `c_name`.
    pub fn runtime(callee: RuntimeCallee, span: Span) -> Self {
        Self::function(Symbol::runtime(callee), span)
    }

    /// What the runtime function this operand names does; `None` for any
    /// other operand.
    pub fn called_runtime_effect(&self) -> Option<&RuntimeEffect> {
        self.called_symbol()?.runtime_effect()
    }

    /// The C name of the runtime function this operand names; `None` for any
    /// other operand.
    pub fn called_runtime_name(&self) -> Option<&str> {
        self.called_symbol()?.runtime_name()
    }

    /// The function this operand names; `None` for a constant and for a
    /// place, whose callee is only known at run time.
    pub fn called_symbol(&self) -> Option<&Symbol> {
        match self {
            Operand::Function(function) => Some(&function.symbol),
            Operand::Move(_) | Operand::Copy(_) | Operand::Constant(_) => None,
        }
    }

    /// The whole local this operand reads, whether spelled as a copy or a
    /// move; `None` for a projected place, a constant and a function.
    pub fn bare_local(&self) -> Option<Local> {
        self.place()
            .filter(|place| place.projection.is_empty())
            .map(|place| place.local)
    }

    /// The place this operand reads, whether spelled as a copy or a move;
    /// `None` for a constant and a function.
    pub fn place(&self) -> Option<&Place> {
        match self {
            Operand::Copy(place) | Operand::Move(place) => Some(place),
            Operand::Constant(_) | Operand::Function(_) => None,
        }
    }

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
            Operand::Function(function) => &function.ty,
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
    /// - `Field(i)` on custom `Struct`/`Class` types → the field's type, as
    ///   [`crate::mir::instantiation::field_type`] reads it
    /// - `Field(i)` on `Function` (closure) → unresolvable: a closure's captures
    ///   are read only where its body opens its environment
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
            Operand::Function(function) => Some(MirType::from_type_kind(&function.ty.kind)),
            Operand::Move(place) | Operand::Copy(place) => resolve_place_mir_type(place, body),
        }
    }
}

/// Resolves the MIR type of a place through its projections.
///
/// Mirrors the logic in `src/mir/optimization/rc_insertion.rs::place_is_counted` to resolve
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
            PlaceElem::Index(_, _) => {
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
                        let field_ty = crate::mir::instantiation::field_type(
                            body.rc_classifier.definitions(),
                            body.generics.instance.as_ref(),
                            name,
                            None,
                            *i,
                        )?;
                        MirType::from_type_kind(&field_ty.kind)
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
            Operand::Function(function) => write!(f, "fn {}", function.symbol),
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

/// A function named as an operand, by the symbol it is compiled under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionConstant {
    pub span: Span,
    pub ty: Type,
    pub symbol: Symbol,
}
