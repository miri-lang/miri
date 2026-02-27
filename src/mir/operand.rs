use crate::ast::literal::Literal;
use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::body::Body;
use crate::mir::place::Place;
use std::fmt;

/// An operand for an Rvalue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Operand {
    /// Moves the value out of the place.
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
    pub fn ty<'a>(&'a self, body: &'a Body) -> &'a Type {
        match self {
            Operand::Move(place) | Operand::Copy(place) => &body.local_decls[place.local.0].ty,
            Operand::Constant(c) => &c.ty,
        }
    }
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
