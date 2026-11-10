// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What the records of the runtime entry points say: the stdlib's `runtime`
//! declarations and the compiler's own table, read the way every stage reads
//! them.

use crate::mir::utils::{every_runtime_record, runtime_callee};
use miri::ast::types::BuiltinCollectionKind;
use miri::mir::compiler_intrinsics::copy_on_write_of;
use miri::runtime_effects::{ParamUse, RuntimeEffect};
use miri::runtime_fns::rt;

/// What the entry point `name` is recorded to do.
fn record(name: &str) -> RuntimeEffect {
    runtime_callee(name).effect().clone()
}

#[test]
fn every_collection_kind_has_its_copy_on_write_entry_point() {
    for (kind, name) in [
        (BuiltinCollectionKind::List, rt::LIST_COW),
        (BuiltinCollectionKind::Set, rt::SET_COW),
        (BuiltinCollectionKind::Map, rt::MAP_COW),
        (BuiltinCollectionKind::Array, rt::ARRAY_COW),
    ] {
        let cow = copy_on_write_of(kind);
        assert_eq!(cow.name(), name);
        assert!(cow.effect().is_copy_on_write(), "{name} copies on write");
    }
    for other in [rt::LIST_CLONE, rt::RC_IS_SHARED, rt::LIST_PUSH] {
        assert!(
            !record(other).is_copy_on_write(),
            "{other} is not a copy-on-write entry point"
        );
    }
}

#[test]
fn only_the_collection_clones_hand_back_a_copy() {
    for clone in [
        rt::LIST_CLONE,
        rt::MAP_CLONE,
        rt::SET_CLONE,
        rt::ARRAY_CLONE,
    ] {
        assert!(
            record(clone).returns_copy(),
            "{clone} allocates a fresh block"
        );
    }
    for other in [rt::LIST_COW, rt::RC_IS_SHARED, rt::LIST_PUSH] {
        assert!(
            !record(other).returns_copy(),
            "{other} may hand back a shared value"
        );
    }
}

#[test]
fn only_calls_that_cannot_write_a_header_leave_every_header_alone() {
    for reader in [
        rt::RC_IS_SHARED,
        rt::RC_STATS_DUP,
        rt::RC_STATS_FREE,
        rt::RC_STATS_COW_COPY,
    ] {
        assert!(record(reader).leaves_headers, "{reader} writes no header");
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
            !record(writer).leaves_headers,
            "{writer} may write a collection header"
        );
    }
}

/// Every collection copy hands back its argument or a fresh copy as long as
/// the original; no writer is among them.
#[test]
fn the_collection_copies_and_nothing_else_copy_a_collection() {
    for copy in [
        rt::LIST_COW,
        rt::LIST_CLONE,
        rt::ARRAY_COW,
        rt::ARRAY_CLONE,
        rt::MAP_COW,
        rt::MAP_CLONE,
        rt::SET_COW,
        rt::SET_CLONE,
    ] {
        let effect = record(copy);
        assert!(effect.copies_collection(), "{copy} copies a collection");
        assert!(effect.preserves_length(), "{copy} keeps the length");
    }
    for other in [rt::LIST_PUSH, rt::LIST_SET, rt::RC_IS_SHARED] {
        assert!(
            !record(other).copies_collection(),
            "{other} is not a collection copy"
        );
    }
}

/// The receiver check reads a count and nothing else: it keeps no reference,
/// hands nothing back and leaves every header as it was, so no pass may take
/// it for a copy-on-write that redefines the receiver.
#[test]
fn the_receiver_check_only_reads_the_count_it_answers_for() {
    let check = record(rt::RECEIVER_UNIQUE);
    assert!(check.reads_count && check.insists_on_unique());
    assert!(check.only_reads(0));
    assert!(check.leaves_headers);
    assert!(!check.is_copy_on_write());
    assert!(!check.preserves_length());
    assert!(!check.returns_copy());
    assert!(check.handed_over_positions().is_empty());
    let question = record(rt::RC_IS_SHARED);
    assert!(question.reads_count && question.asks_whether_shared());
    assert!(!question.insists_on_unique());
    for other in [rt::LIST_COW, rt::LIST_LEN, rt::LIST_PUSH] {
        assert!(!record(other).reads_count, "{other} reads no count");
    }
}

#[test]
fn every_collection_writer_writes_its_receiver_in_place() {
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
        assert!(
            record(writer).writes_in_place(0),
            "{writer} writes its receiver"
        );
    }
}

/// A stored element is taken over and a looked-up one only read; neither,
/// nor a closure the runtime calls, is written in place.
#[test]
fn elements_and_closures_are_not_written_in_place() {
    for (name, position) in [
        (rt::LIST_PUSH, 1),
        (rt::LIST_INSERT, 2),
        (rt::LIST_SET, 2),
        (rt::ARRAY_SET_VAL, 2),
        (rt::MAP_SET, 1),
        (rt::MAP_SET, 2),
        (rt::SET_ADD, 1),
    ] {
        let effect = record(name);
        assert!(
            !effect.writes_in_place(position),
            "{name} stores {position}"
        );
        assert!(
            effect.handed_over_positions().contains(&position),
            "{name} keeps argument {position}"
        );
    }
    for (name, position) in [
        (rt::SET_CONTAINS, 1),
        (rt::MAP_REMOVE, 1),
        (rt::SET_REMOVE, 1),
    ] {
        assert!(
            record(name).only_reads(position),
            "{name} only reads {position}"
        );
    }
    assert_eq!(
        record(rt::MAP_SET).stored_entry(),
        Some((1, 2)),
        "a map stores a key and a value as one entry"
    );
    assert!(
        !record(rt::LIST_SORT_BY).writes_in_place(1),
        "a closure is called"
    );
}

#[test]
fn readers_copies_and_count_questions_write_nothing_in_place() {
    for name in [
        rt::LIST_LEN,
        rt::MAP_GET,
        rt::MAP_GET_CHECKED,
        rt::SET_ELEMENT_AT,
        rt::LIST_CLONE,
        rt::LIST_COW,
        rt::MAP_COW,
        rt::ARRAY_COW,
        rt::RC_IS_SHARED,
        rt::RECEIVER_UNIQUE,
        rt::LIST_NEW_FROM_RAW,
        rt::LIST_NEW_FROM_MANAGED_ARRAY,
        rt::ARRAY_SLICE,
    ] {
        assert!(!record(name).writes_in_place(0), "{name} writes nothing");
    }
}

/// What a runtime entry point writes in place it does not keep: the value is
/// lent for the call, so the count it found is the count it leaves, and a
/// place made unique before the call is still unique after it. The one taken
/// argument also written is a string append's receiver, which the call hands
/// back as its result.
#[test]
fn a_runtime_write_keeps_nothing_it_is_handed() {
    for callee in every_runtime_record() {
        let effect = callee.effect();
        for position in effect.handed_over_positions() {
            assert!(
                !effect.writes_in_place(position) || effect.param(position) == ParamUse::Appends,
                "{} both keeps and writes in place argument {position}",
                callee.name()
            );
        }
    }
}

/// An empty collection just built is one nothing else holds: the elision and
/// the unshared-writes verifier both rely on it. Only the constructors of the
/// four collections qualify.
#[test]
fn only_a_collection_constructor_allocates_a_fresh_collection() {
    let constructors = [rt::LIST_NEW, rt::MAP_NEW, rt::SET_NEW, rt::ARRAY_NEW];
    for callee in every_runtime_record() {
        assert_eq!(
            callee.effect().allocates_collection(),
            constructors.contains(&callee.name()),
            "{}",
            callee.name()
        );
    }
}

/// The string, text-pattern, console, file-system and environment entry
/// points copy out of the strings they are handed, so none writes one in
/// place or keeps it: a check that credits a call as writing nothing a caller
/// holds must not stop at one of these.
#[test]
fn the_string_and_io_entry_points_only_read_their_strings() {
    let readers = [
        rt::STRING_CHAR_AT,
        rt::STRING_CODE_AT,
        rt::STRING_INDEX_OF_ANY,
        rt::STRING_LINE_COUNT,
        rt::STRING_LINE_TEXT,
        rt::STRING_NEXT_CHAR_BOUNDARY,
        rt::STRING_PARSE_FLOAT,
        rt::STRING_PARSE_INT,
        rt::STRING_PIECE_COUNT,
        rt::STRING_PIECE_END,
        rt::STRING_REPEAT,
        rt::STRING_REPLACE,
        rt::STRING_SLICE_BYTES,
        rt::STRING_SPLIT,
        rt::STRING_SUBSTRING,
        rt::STRING_TO_LOWER,
        rt::STRING_TO_UPPER,
        rt::STRING_TRIM,
        rt::STRING_TRIM_END,
        rt::STRING_TRIM_START,
        rt::REGEX_COMPILE,
        rt::REGEX_FROM_VALIDATED_PATTERN,
        rt::REGEX_FIND,
        rt::REGEX_FIND_FROM,
        rt::REGEX_MATCHES,
        rt::REGEX_REPLACE,
        rt::PRINT,
        rt::PRINTLN,
        rt::EPRINT,
        rt::EPRINTLN,
        rt::PANIC,
        rt::FS_READ_FILE,
        rt::FS_WRITE_FILE,
        rt::FS_APPEND_FILE,
        rt::FS_CREATE_DIR,
        rt::FS_DELETE,
        rt::FS_EXISTS,
        rt::FS_LIST_DIR,
        rt::ENV_GET,
        rt::ENV_HAS,
        rt::ENV_SET,
    ];
    for name in readers {
        let effect = record(name);
        for position in 0..effect.params.len() {
            assert!(
                effect.only_reads(position),
                "{name} only reads argument {position}"
            );
        }
        assert!(
            effect.handed_over_positions().is_empty(),
            "{name} takes nothing over"
        );
    }
}
