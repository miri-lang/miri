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
    constructed_vtable_symbols, released_by_runtime_class, FilledSlot, VtableFills, VtableInstance,
    VtableLayout, DROP_SLOT,
};

pub use crate::type_checker::context::class_needs_vtable;

pub use crate::ast::implicit_methods::ELEMENT_METHOD_NAMES;
