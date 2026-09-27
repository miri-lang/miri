// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The settled shape of dynamic dispatch: the slot numbering every vtable
//! shares, which vtable a constructed instance points at, which slots of each
//! vtable a program fills, and the methods a container asks of its elements.
//!
//! These are products the pipeline settles once lowering is done and code
//! generation reads as data. How a slot's target is resolved — the rules of
//! method dispatch — stays with MIR lowering.

mod vtable;

pub(crate) use vtable::{constructed_class, dispatched_method_names, takes_vtable_slot};
pub use vtable::{
    constructed_vtable_symbols, FilledSlot, VtableFills, VtableInstance, VtableLayout,
};

pub use crate::type_checker::context::class_needs_vtable;

use crate::ast::types::{EQUALS_METHOD_NAME, ORDERING_METHOD_NAME};

/// The methods a container's runtime thunk asks of two class elements: the
/// ordering it sorts by and the equality it matches by.
pub const ELEMENT_METHOD_NAMES: [&str; 2] = [ORDERING_METHOD_NAME, EQUALS_METHOD_NAME];
