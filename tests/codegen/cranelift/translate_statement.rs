// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::codegen::cranelift::FunctionTranslator;
use miri::mir::{ArgMode, Local, Operand, Place};

/// An `out` argument goes through a slot whatever its type, a managed one
/// as a scalar does, so a new value the callee assigns reaches the caller:
/// the choice reads only the argument's mode and place.
#[test]
fn scalar_out_local_returns_local_for_scalar_out_arg() {
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: Vec::new(),
    });
    let got = FunctionTranslator::out_local_for_arg(&[ArgMode::Out], 0, &arg);
    assert_eq!(got, Some(Local(0)));
}

#[test]
fn scalar_out_local_returns_none_when_flag_unset() {
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: Vec::new(),
    });
    let got = FunctionTranslator::out_local_for_arg(&[ArgMode::Borrowed], 0, &arg);
    assert!(got.is_none());
}

#[test]
fn scalar_out_local_skips_projected_places() {
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: vec![miri::mir::PlaceElem::Field(0)],
    });
    let got = FunctionTranslator::out_local_for_arg(&[ArgMode::Out], 0, &arg);
    assert!(got.is_none());
}
