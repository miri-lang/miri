// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The rule that no vtable slot holds a copy of a generic method.
//!
//! A slot holds one body, and a generic method has one per set of type
//! arguments its calls bind, so vtable layout gives it no slot and every
//! call to it is static. A slot pointing at such a copy would answer a virtual call
//! with the body compiled for one type argument whatever the call passes.

use std::fmt;

use crate::mir::dispatch::VtableFills;
use crate::mir::symbol::Symbol;

/// A filled vtable slot pointing at a copy of a generic method. A slot is no
/// local of any body, so the finding names the vtable and the slot instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotViolation {
    /// The vtable holding the slot.
    pub vtable: String,
    /// The slot's number.
    pub slot: usize,
    /// The method the slot stands for.
    pub method: String,
    /// The copy the slot points at.
    pub symbol: Symbol,
}

impl fmt::Display for SlotViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "slot {} of `{}` holds `{}` ({}), a copy of a generic method compiled at one set of \
             type arguments, which a virtual call would run whatever it passes",
            self.slot, self.vtable, self.method, self.symbol
        )
    }
}

/// Each filled slot of `fills` that points at a copy of a generic method.
pub fn verify_vtable_slots(fills: &VtableFills) -> Vec<SlotViolation> {
    fills
        .iter()
        .flat_map(|(vtable, slots)| slots.iter().map(move |slot| (vtable, slot)))
        .filter(|(_, slot)| slot.symbol.is_generic_method_copy())
        .map(|(vtable, slot)| SlotViolation {
            vtable: vtable.to_string(),
            slot: slot.slot,
            method: slot.method.clone(),
            symbol: slot.symbol.clone(),
        })
        .collect()
}
