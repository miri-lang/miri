// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::BuiltinCollectionKind;
use miri::runtime_fns::{
    cow_fn, is_copy_on_write, leaves_collection_headers_unchanged, returns_unshared, rt,
};

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

#[test]
fn is_copy_on_write_names_every_collection_kind_entry_point_and_nothing_else() {
    for kind in [
        BuiltinCollectionKind::List,
        BuiltinCollectionKind::Set,
        BuiltinCollectionKind::Map,
        BuiltinCollectionKind::Array,
    ] {
        let cow = cow_fn(kind).expect("every collection kind has a copy-on-write entry point");
        assert!(
            is_copy_on_write(cow),
            "{cow} is a copy-on-write entry point"
        );
    }
    for other in [rt::LIST_CLONE, rt::RC_IS_SHARED, rt::LIST_PUSH] {
        assert!(
            !is_copy_on_write(other),
            "{other} is not a copy-on-write entry point"
        );
    }
}

#[test]
fn leaves_collection_headers_unchanged_names_only_calls_that_cannot_write_a_header() {
    for reader in [
        rt::RC_IS_SHARED,
        rt::RC_STATS_DUP,
        rt::RC_STATS_FREE,
        rt::RC_STATS_COW_COPY,
    ] {
        assert!(
            leaves_collection_headers_unchanged(reader),
            "{reader} writes no collection header"
        );
    }
    for writer in [
        rt::LIST_PUSH,
        rt::LIST_COW,
        rt::LIST_CLONE,
        rt::LIST_SET,
        rt::LIST_INSERT,
        rt::ARRAY_COW,
        rt::ARRAY_SORT,
    ] {
        assert!(
            !leaves_collection_headers_unchanged(writer),
            "{writer} may write a collection header"
        );
    }
}
