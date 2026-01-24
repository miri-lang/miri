use crate::ast::literal::Literal;
use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::body::Body;
use crate::mir::place::{Place, PlaceElem};
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
    pub fn ty(&self, body: &Body) -> Type {
        match self {
            Operand::Move(place) | Operand::Copy(place) => {
                let ty = body.local_decls[place.local.0].ty.clone();
                for elem in &place.projection {
                    match elem {
                        PlaceElem::Deref => {
                            // TODO: Implement Deref type resolution if we support pointers
                        }
                        PlaceElem::Field(_idx) => {
                            // TODO: Implement field type resolution for tuples/structs
                            if let crate::ast::types::TypeKind::Tuple(_elements) = &ty.kind {}
                        }
                        PlaceElem::Index(_) => {
                            // TODO: Array/List element type
                        }
                    }
                }
                ty
            }
            Operand::Constant(c) => c.ty.clone(),
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
