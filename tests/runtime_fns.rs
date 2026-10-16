// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::BuiltinCollectionKind;
use miri::runtime_fns::{cow_fn, returns_unshared, rt};

#[test]
fn cow_fn_returns_correct_values() {
    assert_eq!(cow_fn(BuiltinCollectionKind::List), Some(rt::LIST_COW));
    assert_eq!(cow_fn(BuiltinCollectionKind::Set), Some(rt::SET_COW));
    assert_eq!(cow_fn(BuiltinCollectionKind::Map), Some(rt::MAP_COW));
    assert_eq!(cow_fn(BuiltinCollectionKind::Array), Some(rt::ARRAY_COW));
}

#[test]
fn returns_unshared_lists_only_the_collection_clones() {
    for clone in [
        rt::LIST_CLONE,
        rt::MAP_CLONE,
        rt::SET_CLONE,
        rt::ARRAY_CLONE,
    ] {
        assert!(returns_unshared(clone), "{clone} allocates a fresh block");
    }
    for other in [rt::LIST_COW, rt::RC_IS_SHARED, rt::LIST_PUSH] {
        assert!(
            !returns_unshared(other),
            "{other} may hand back a shared value"
        );
    }
}
