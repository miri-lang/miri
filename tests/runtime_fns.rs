// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::BuiltinCollectionKind;
use miri::runtime_fns::{cow_fn, rt};

#[test]
fn cow_fn_returns_correct_values() {
    assert_eq!(cow_fn(BuiltinCollectionKind::List), Some(rt::LIST_COW));
    assert_eq!(cow_fn(BuiltinCollectionKind::Set), Some(rt::SET_COW));
    assert_eq!(cow_fn(BuiltinCollectionKind::Map), Some(rt::MAP_COW));
    assert_eq!(cow_fn(BuiltinCollectionKind::Array), None);
}
