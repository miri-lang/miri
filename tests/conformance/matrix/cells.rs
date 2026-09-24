// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The axes of the matrix and the outcome each cell must reach.
//!
//! A cell is one element type in one slot, under one operation, in one
//! context. Its name — `slot/type/operation/context` — is stable, so a bug
//! report or `KNOWN_RED.toml` can quote it and `grep` finds it.

use super::types::{ElementType, Value, ELEMENT_TYPES};

/// Where a value lives while the operation runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Local,
    StructField,
    ClassField,
    /// A field a generic class declares, reached through a subclass that
    /// `extends` it, beside a scalar and a managed field.
    InheritedField,
    /// The first payload of a two-payload enum variant.
    EnumPayload,
    ListElement,
    ArrayElement,
    SetElement,
    MapKey,
    MapValue,
    Parameter,
    Return,
    ClosureCapture,
    /// The field of a generic struct instantiated at the type.
    GenericField,
}

/// What is done to the value in its slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operation {
    StoreRead,
    Overwrite,
    /// Overwrites with the second value spelled out at the write site, so the
    /// written expression takes its type from the slot.
    WriteLiteral,
    Equality,
    Contains,
    /// Adds two values `==` calls equal to a keyed collection and counts it.
    Dedup,
    Sort,
    Pop,
    First,
    Last,
    RemoveAt,
    Reversed,
    /// Builds the collection from both elements at once.
    Construct,
    /// Reads a map value through `get` rather than indexing.
    Get,
    CompoundAssign,
    /// Adds two values each returned from a call.
    Add,
    Render,
}

/// The code the operation is written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Context {
    /// A plain function over the concrete type.
    Monomorphic,
    /// A generic function instantiated at the type.
    GenericFunction,
    /// A method of a generic class instantiated at the type.
    GenericClassMethod,
    /// A generic base-class method reached through a subclass that `extends`
    /// the base at the type.
    InheritedMethod,
    /// A default method of a generic trait, reached through a non-generic
    /// class that implements the trait at the type.
    TraitDefault,
}

pub const SLOTS: &[Slot] = &[
    Slot::Local,
    Slot::StructField,
    Slot::ClassField,
    Slot::InheritedField,
    Slot::EnumPayload,
    Slot::ListElement,
    Slot::ArrayElement,
    Slot::SetElement,
    Slot::MapKey,
    Slot::MapValue,
    Slot::Parameter,
    Slot::Return,
    Slot::ClosureCapture,
    Slot::GenericField,
];

pub const OPERATIONS: &[Operation] = &[
    Operation::StoreRead,
    Operation::Overwrite,
    Operation::WriteLiteral,
    Operation::Equality,
    Operation::Contains,
    Operation::Dedup,
    Operation::Sort,
    Operation::Pop,
    Operation::First,
    Operation::Last,
    Operation::RemoveAt,
    Operation::Reversed,
    Operation::Construct,
    Operation::Get,
    Operation::CompoundAssign,
    Operation::Add,
    Operation::Render,
];

pub const CONTEXTS: &[Context] = &[
    Context::Monomorphic,
    Context::GenericFunction,
    Context::GenericClassMethod,
    Context::InheritedMethod,
    Context::TraitDefault,
];

impl Slot {
    pub fn token(self) -> &'static str {
        match self {
            Slot::Local => "local",
            Slot::StructField => "struct_field",
            Slot::ClassField => "class_field",
            Slot::InheritedField => "inherited_field",
            Slot::EnumPayload => "enum_payload",
            Slot::ListElement => "list_element",
            Slot::ArrayElement => "array_element",
            Slot::SetElement => "set_element",
            Slot::MapKey => "map_key",
            Slot::MapValue => "map_value",
            Slot::Parameter => "parameter",
            Slot::Return => "return",
            Slot::ClosureCapture => "closure_capture",
            Slot::GenericField => "generic_field",
        }
    }

    /// Whether the slot hashes or compares the value to find it again.
    fn is_keyed(self) -> bool {
        matches!(self, Slot::SetElement | Slot::MapKey)
    }

    /// Whether the slot keeps its value in a place that can be assigned.
    pub fn is_assignable(self) -> bool {
        match self {
            Slot::Local
            | Slot::StructField
            | Slot::ClassField
            | Slot::InheritedField
            | Slot::GenericField
            | Slot::ListElement
            | Slot::ArrayElement
            | Slot::MapValue
            | Slot::ClosureCapture => true,
            Slot::EnumPayload
            | Slot::SetElement
            | Slot::MapKey
            | Slot::Parameter
            | Slot::Return => false,
        }
    }
}

impl Operation {
    pub fn token(self) -> &'static str {
        match self {
            Operation::StoreRead => "store_read",
            Operation::Overwrite => "overwrite",
            Operation::WriteLiteral => "write_literal",
            Operation::Equality => "eq",
            Operation::Contains => "contains",
            Operation::Dedup => "dedup",
            Operation::Sort => "sort",
            Operation::Pop => "pop",
            Operation::First => "first",
            Operation::Last => "last",
            Operation::RemoveAt => "remove_at",
            Operation::Reversed => "reversed",
            Operation::Construct => "construct",
            Operation::Get => "get",
            Operation::CompoundAssign => "compound_assign",
            Operation::Add => "add",
            Operation::Render => "render",
        }
    }

    /// Whether the operation hands back a count rather than a value.
    pub fn counts(self) -> bool {
        matches!(
            self,
            Operation::Equality | Operation::Contains | Operation::Dedup
        )
    }
}

impl Context {
    pub fn token(self) -> &'static str {
        match self {
            Context::Monomorphic => "mono",
            Context::GenericFunction => "generic_fn",
            Context::GenericClassMethod => "generic_class",
            Context::InheritedMethod => "inherited",
            Context::TraitDefault => "trait_default",
        }
    }

    pub fn is_generic(self) -> bool {
        !matches!(self, Context::Monomorphic)
    }

    /// Whether the context is judged in `slot`. A trait default body is
    /// compiled per implementor whatever the slot, so the local slot alone
    /// covers it; running it everywhere would only repeat the local verdict
    /// at the cost of a program per crashing cell.
    fn covers(self, slot: Slot) -> bool {
        self != Context::TraitDefault || slot == Slot::Local
    }
}

/// What a cell's program prints for it when the cell is correct.
pub enum Expected {
    /// The operation hands back a value of the element type.
    Value(&'static Value),
    /// The operation counts: `1` means "found `a`, not `b`", or "one entry".
    Count,
    /// The operation renders the value through string interpolation.
    Text(&'static str),
}

/// The verdict a cell must reach.
pub enum Outcome {
    /// The program compiles, runs cleanly and prints the expected line.
    Runs(Expected),
    /// The compiler refuses the program with this diagnostic code.
    Refused(&'static str),
}

/// One cell of the matrix.
pub struct Cell {
    pub name: String,
    pub slot: Slot,
    pub ty: &'static ElementType,
    pub operation: Operation,
    pub context: Context,
    pub outcome: Outcome,
}

/// Every cell in one slot and context, in a stable order.
pub fn cells_for(slot: Slot, context: Context) -> Vec<Cell> {
    let mut cells = Vec::new();
    for ty in ELEMENT_TYPES {
        for &operation in OPERATIONS {
            if let Some(outcome) = outcome_of(slot, ty, operation, context) {
                cells.push(Cell {
                    name: cell_name(slot, ty, operation, context),
                    slot,
                    ty,
                    operation,
                    context,
                    outcome,
                });
            }
        }
    }
    cells
}

pub fn cell_name(slot: Slot, ty: &ElementType, operation: Operation, context: Context) -> String {
    format!(
        "{}/{}/{}/{}",
        slot.token(),
        ty.token,
        operation.token(),
        context.token()
    )
}

/// What `operation` on `ty` in `slot` must do in `context`, or `None` when
/// the cell does not exist — the operation has no meaning in the slot
/// (sorting a local, popping a map key) or the context is not judged there.
///
/// Interpolation needs the concrete type to pick a conversion, so a generic
/// body may not interpolate its type parameter whatever it is instantiated at.
pub fn outcome_of(
    slot: Slot,
    ty: &'static ElementType,
    operation: Operation,
    context: Context,
) -> Option<Outcome> {
    if !applies(slot, operation, context) || !context.covers(slot) {
        return None;
    }
    let unrenderable = !ty.renderable || context.is_generic();
    if operation == Operation::Render && unrenderable {
        return Some(Outcome::Refused(refusal::NOT_RENDERABLE));
    }
    if slot.is_keyed() && !ty.keyable {
        return Some(Outcome::Refused(refusal::UNKEYABLE));
    }
    Some(match operation {
        Operation::Equality | Operation::Contains | Operation::Dedup if !ty.equatable => {
            Outcome::Refused(refusal::NOT_EQUATABLE)
        }
        Operation::Sort if !ty.orderable => Outcome::Refused(refusal::NOT_ORDERABLE),
        Operation::CompoundAssign | Operation::Add => match &ty.sum {
            Some(sum) => Outcome::Runs(Expected::Value(sum)),
            None => Outcome::Refused(refusal::NOT_ADDABLE),
        },
        Operation::Render => Outcome::Runs(Expected::Text(ty.a.rendered)),
        Operation::Equality | Operation::Contains | Operation::Dedup => {
            Outcome::Runs(Expected::Count)
        }
        Operation::Overwrite
        | Operation::WriteLiteral
        | Operation::Pop
        | Operation::Last
        | Operation::Reversed => Outcome::Runs(Expected::Value(&ty.b)),
        Operation::StoreRead
        | Operation::Sort
        | Operation::First
        | Operation::RemoveAt
        | Operation::Construct
        | Operation::Get => Outcome::Runs(Expected::Value(&ty.a)),
    })
}

/// The diagnostic codes a refused cell must carry.
pub mod refusal {
    pub const UNKEYABLE: &str = "MER_TYP_002";
    pub const NOT_EQUATABLE: &str = "MER_TYP_002";
    pub const NOT_ORDERABLE: &str = "MER_TYP_075";
    pub const NOT_ADDABLE: &str = "MER_TYP_002";
    pub const NOT_RENDERABLE: &str = "MER_TYP_067";
}

/// Whether `operation` is meaningful for a value held in `slot`.
///
/// A literal can only be spelled where its type is concrete, so writing one
/// is judged in the monomorphic context alone.
fn applies(slot: Slot, operation: Operation, context: Context) -> bool {
    match operation {
        Operation::StoreRead | Operation::Equality | Operation::Render => true,
        Operation::Overwrite | Operation::CompoundAssign => slot.is_assignable(),
        Operation::WriteLiteral => {
            context == Context::Monomorphic
                && (slot == Slot::Return || (slot.is_assignable() && slot != Slot::ClosureCapture))
        }
        Operation::Contains => matches!(
            slot,
            Slot::ListElement | Slot::ArrayElement | Slot::SetElement | Slot::MapKey
        ),
        Operation::Dedup => slot.is_keyed(),
        Operation::Sort | Operation::First | Operation::Last | Operation::Construct => {
            matches!(slot, Slot::ListElement | Slot::ArrayElement)
        }
        Operation::Pop | Operation::RemoveAt | Operation::Reversed => slot == Slot::ListElement,
        Operation::Get => slot == Slot::MapValue,
        Operation::Add => slot == Slot::Return,
    }
}
