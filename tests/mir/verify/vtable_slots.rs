// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A vtable slot holds one body, so it never holds a copy of a generic
//! method compiled at the method's own type arguments.

use miri::ast::types::{Type, TypeKind};
use miri::mir::lowering::vtable_demand::{FilledSlot, VtableFills};
use miri::mir::symbol::Symbol;
use miri::mir::verify::verify_vtable_slots;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Default::default())
}

/// Fills of one vtable `miri.Impl.$vtable` with `symbol` in slot 1.
fn fills_with(symbol: Symbol) -> VtableFills {
    let slot = FilledSlot {
        slot: 1,
        method: "pick".to_string(),
        symbol,
    };
    [("miri.Impl.$vtable".to_string(), vec![slot])]
        .into_iter()
        .collect()
}

/// A slot holding a generic method's copy is reported, naming the vtable,
/// the slot and the method.
#[test]
fn a_slot_holding_a_generic_method_copy_is_reported() {
    let copy = Symbol::method("Impl", &[], "pick", &[ty(TypeKind::Int)]);
    let found = verify_vtable_slots(&fills_with(copy));
    assert_eq!(found.len(), 1, "{found:?}");
    let message = found[0].to_string();
    assert!(
        message.contains("slot 1 of `miri.Impl.$vtable` holds `pick`"),
        "{message}"
    );
}

/// A slot holding a method compiled with no type arguments of its own is
/// what every slot holds, and is not reported.
#[test]
fn a_slot_holding_a_plain_method_is_not_reported() {
    let plain = Symbol::method("Impl", &[ty(TypeKind::Int)], "pick", &[]);
    assert!(verify_vtable_slots(&fills_with(plain)).is_empty());
}
