// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::BuiltinCollectionKind;
use miri::runtime_fns::{
    cow_fn, is_copy_on_write, leaves_collection_headers_unchanged, only_reads_argument,
    preserves_length, reads_reference_count, returns_unshared, rt, taken_argument_positions,
    writes_in_place,
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

/// The receiver check reads a count and nothing else: it keeps no reference,
/// hands nothing back and leaves every header as it was, so no pass may take
/// it for a copy-on-write that redefines the receiver.
#[test]
fn the_receiver_check_only_reads_the_count_it_answers_for() {
    assert!(reads_reference_count(rt::RECEIVER_UNIQUE));
    assert!(reads_reference_count(rt::RC_IS_SHARED));
    assert!(only_reads_argument(rt::RECEIVER_UNIQUE, 0));
    assert!(leaves_collection_headers_unchanged(rt::RECEIVER_UNIQUE));
    assert!(!is_copy_on_write(rt::RECEIVER_UNIQUE));
    assert!(!preserves_length(rt::RECEIVER_UNIQUE));
    assert!(!returns_unshared(rt::RECEIVER_UNIQUE));
    assert!(taken_argument_positions(rt::RECEIVER_UNIQUE).is_empty());
    for other in [rt::LIST_COW, rt::LIST_LEN, rt::LIST_PUSH] {
        assert!(!reads_reference_count(other), "{other} reads no count");
    }
}

#[test]
fn writes_in_place_names_the_receiver_of_every_collection_writer() {
    for writer in [
        rt::LIST_PUSH,
        rt::LIST_SET,
        rt::LIST_INSERT,
        rt::LIST_REMOVE,
        rt::LIST_CLEAR,
        rt::LIST_RESERVE,
        rt::LIST_REVERSE,
        rt::LIST_SORT,
        rt::LIST_SORT_BY,
        rt::MAP_SET,
        rt::MAP_REMOVE,
        rt::MAP_CLEAR,
        rt::SET_ADD,
        rt::SET_REMOVE,
        rt::SET_CLEAR,
        rt::ARRAY_SET_VAL,
        rt::ARRAY_SORT,
    ] {
        assert!(writes_in_place(writer, 0), "{writer} writes its receiver");
    }
}

#[test]
fn writes_in_place_leaves_out_elements_closures_and_numbers() {
    assert!(
        !writes_in_place(rt::LIST_PUSH, 1),
        "a pushed element is stored"
    );
    assert!(
        !writes_in_place(rt::LIST_INSERT, 2),
        "an inserted element is stored"
    );
    assert!(!writes_in_place(rt::MAP_SET, 1), "a key is stored");
    assert!(!writes_in_place(rt::MAP_SET, 2), "a value is stored");
    assert!(
        !writes_in_place(rt::SET_CONTAINS, 1),
        "a looked-up element is read"
    );
    assert!(!writes_in_place(rt::LIST_SORT_BY, 1), "a closure is called");
}

#[test]
fn writes_in_place_leaves_out_readers_copies_and_count_questions() {
    for (reader, position) in [
        (rt::LIST_LEN, 0),
        (rt::MAP_GET, 0),
        (rt::MAP_GET_CHECKED, 0),
        (rt::SET_ELEMENT_AT, 0),
        (rt::LIST_CLONE, 0),
        (rt::LIST_COW, 0),
        (rt::MAP_COW, 0),
        (rt::ARRAY_COW, 0),
        (rt::RC_IS_SHARED, 0),
        (rt::RECEIVER_UNIQUE, 0),
        (rt::LIST_NEW_FROM_RAW, 0),
        (rt::LIST_NEW_FROM_MANAGED_ARRAY, 0),
        (rt::ARRAY_SLICE, 0),
    ] {
        assert!(
            !writes_in_place(reader, position),
            "{reader} does not write argument {position} in place"
        );
    }
}

/// A runtime entry point nothing classifies is taken to write what it is
/// handed, so a new one is copied for until a table says it only reads.
#[test]
fn writes_in_place_fails_closed_on_an_unknown_entry_point() {
    assert!(writes_in_place("miri_rt_not_declared_anywhere", 0));
    assert!(writes_in_place("miri_rt_not_declared_anywhere", 3));
}

/// What a runtime entry point writes in place it does not keep: the value is
/// lent for the call, so the count it found is the count it leaves, and a
/// place made unique before the call is still unique after it. The tables
/// can say this much: an argument the entry point keeps is one it takes
/// over, and the only taken argument it also writes is a string append's
/// receiver, which the call hands back as its result.
#[test]
fn a_runtime_write_keeps_nothing_it_is_handed() {
    let hands_back_its_receiver = [
        rt::STRING_APPEND,
        rt::STRING_APPEND_INT,
        rt::STRING_APPEND_UINT,
    ];
    for &name in rt::ALL {
        for &position in taken_argument_positions(name) {
            let is_written = writes_in_place(name, position);
            assert!(
                !is_written || (position == 0 && hands_back_its_receiver.contains(&name)),
                "{name} both keeps and writes in place argument {position}"
            );
        }
    }
}
