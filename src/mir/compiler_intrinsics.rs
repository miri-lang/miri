// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What each runtime entry point only the compiler calls does to its
//! arguments and its result.
//!
//! A runtime entry point a program can call is declared in the stdlib, and
//! its declaration states its effects. The entry points listed here have no
//! declaration: the compiler emits each call itself — a copy-on-write, a drop
//! hook registration, a failure report, a number formatted into a string — so
//! the compiler states their effects, here and nowhere else, one record per
//! entry point. An entry point both declared and listed here, or emitted and
//! neither, is a compiler error rather than a guess.
//!
//! Each record lists the parameters as a MIR call passes them, before the
//! element ABI spells an element argument as an address and a byte count.

use std::rc::Rc;

use crate::ast::types::BuiltinCollectionKind;

use crate::mir::residency::{ACQUIRE_FN, READBACK_FN, RELEASE_FN, TRANSFER_FN, UPLOAD_FN};
use crate::mir::symbol::RuntimeCallee;
use crate::runtime_effects::{ElementRole, ParamUse, ResultUse, RuntimeEffect};
use crate::runtime_fns::rt;

/// One entry point's record, in the compact form the table is written in.
pub struct Intrinsic {
    name: &'static str,
    params: &'static [ParamUse],
    result: ResultUse,
    flags: Flags,
}

/// The facts of a record beyond its parameters and its result.
#[derive(Clone, Copy)]
struct Flags {
    diverges: bool,
    leaves_headers: bool,
    reads_count: bool,
    matches_receiver: bool,
}

const PLAIN: Flags = Flags {
    diverges: false,
    leaves_headers: false,
    reads_count: false,
    matches_receiver: false,
};
const DIVERGES: Flags = Flags {
    diverges: true,
    ..PLAIN
};
const LEAVES_HEADERS: Flags = Flags {
    leaves_headers: true,
    ..PLAIN
};
const COUNT_QUESTION: Flags = Flags {
    leaves_headers: true,
    reads_count: true,
    ..PLAIN
};

const S: ParamUse = ParamUse::Scalar;
const R: ParamUse = ParamUse::Reads;
const W: ParamUse = ParamUse::Writes;
const OWNED: ResultUse = ResultUse::Owned;

const fn entry(name: &'static str, params: &'static [ParamUse]) -> Intrinsic {
    Intrinsic {
        name,
        params,
        result: OWNED,
        flags: PLAIN,
    }
}

const fn flagged(name: &'static str, params: &'static [ParamUse], flags: Flags) -> Intrinsic {
    Intrinsic {
        name,
        params,
        result: OWNED,
        flags,
    }
}

/// Copy-on-write: takes its receiver over and hands back the receiver, or a
/// copy only the caller holds.
const fn copy_on_write(name: &'static str) -> Intrinsic {
    entry(name, &[ParamUse::CopiesOnWrite])
}

pub static LIST_COW: Intrinsic = copy_on_write(rt::LIST_COW);
pub static MAP_COW: Intrinsic = copy_on_write(rt::MAP_COW);
pub static SET_COW: Intrinsic = copy_on_write(rt::SET_COW);
pub static ARRAY_COW: Intrinsic = copy_on_write(rt::ARRAY_COW);
pub static STRING_APPEND: Intrinsic = entry(rt::STRING_APPEND, &[ParamUse::Appends, R]);
pub static STRING_APPEND_INT: Intrinsic = entry(rt::STRING_APPEND_INT, &[ParamUse::Appends, S]);
pub static STRING_APPEND_UINT: Intrinsic = entry(rt::STRING_APPEND_UINT, &[ParamUse::Appends, S]);
pub static MAP_GET_CHECKED: Intrinsic = Intrinsic {
    name: rt::MAP_GET_CHECKED,
    params: &[R, ParamUse::LooksUp(ElementRole::Key)],
    result: ResultUse::BorrowedElement,
    flags: Flags {
        matches_receiver: true,
        ..PLAIN
    },
};
pub static LIST_NEW_FROM_RAW: Intrinsic = entry(rt::LIST_NEW_FROM_RAW, &[R, S, S]);
pub static LIST_NEW_FROM_MANAGED_ARRAY: Intrinsic =
    entry(rt::LIST_NEW_FROM_MANAGED_ARRAY, &[R, S, S]);
pub static ARRAY_SLICE: Intrinsic = entry(rt::ARRAY_SLICE, &[R, S, S]);
pub static FLOAT_HASH: Intrinsic = entry(rt::FLOAT_HASH, &[S]);
pub static FLOAT_TO_STRING: Intrinsic = entry(rt::FLOAT_TO_STRING, &[S]);
pub static F32_TO_STRING: Intrinsic = entry(rt::F32_TO_STRING, &[S]);
pub static INT_TO_STRING: Intrinsic = entry(rt::INT_TO_STRING, &[S]);
pub static UINT_TO_STRING: Intrinsic = entry(rt::UINT_TO_STRING, &[S]);
pub static BOOL_TO_STRING: Intrinsic = entry(rt::BOOL_TO_STRING, &[S]);
pub static I128_TO_STRING: Intrinsic = entry(rt::I128_TO_STRING, &[W]);
pub static U128_TO_STRING: Intrinsic = entry(rt::U128_TO_STRING, &[W]);
pub static FORMAT_FIXED: Intrinsic = entry(rt::FORMAT_FIXED, &[S, S]);
pub static FORMAT_PAD: Intrinsic = entry(rt::FORMAT_PAD, &[W, S, S, S, S]);
pub static FORMAT_RADIX: Intrinsic = entry(rt::FORMAT_RADIX, &[S, S, S, S]);
pub static ASSERT_PANICS: Intrinsic = entry(rt::ASSERT_PANICS, &[W, W, W, S, S]);
pub static READBACK: Intrinsic = entry(READBACK_FN, &[S, W]);
pub static UPLOAD: Intrinsic = entry(UPLOAD_FN, &[S, W]);
pub static ACQUIRE: Intrinsic = entry(ACQUIRE_FN, &[S]);
pub static TRANSFER: Intrinsic = entry(TRANSFER_FN, &[S, S]);
pub static RELEASE: Intrinsic = entry(RELEASE_FN, &[S]);
pub static RECEIVER_UNIQUE: Intrinsic = flagged(rt::RECEIVER_UNIQUE, &[R], COUNT_QUESTION);
pub static RC_IS_SHARED: Intrinsic =
    flagged(rt::RC_IS_SHARED, &[ParamUse::CountOf], COUNT_QUESTION);
pub static RC_STATS_DUP: Intrinsic = flagged(rt::RC_STATS_DUP, &[], LEAVES_HEADERS);
pub static RC_STATS_FREE: Intrinsic = flagged(rt::RC_STATS_FREE, &[], LEAVES_HEADERS);
pub static RC_STATS_COW_COPY: Intrinsic = flagged(rt::RC_STATS_COW_COPY, &[], LEAVES_HEADERS);
pub static ASSERT_FAIL: Intrinsic = flagged(rt::ASSERT_FAIL, &[W, W, W, S, S], DIVERGES);
pub static ASSERT_EQ_FAIL: Intrinsic = flagged(rt::ASSERT_EQ_FAIL, &[W, W, W, W, S, S], DIVERGES);
pub static ASSERT_NE_FAIL: Intrinsic = flagged(rt::ASSERT_NE_FAIL, &[W, W, W, S, S], DIVERGES);
pub static METHOD_NOT_CHECKED_PANIC: Intrinsic =
    flagged(rt::METHOD_NOT_CHECKED_PANIC, &[], DIVERGES);
pub static GUARD_FAILED_PANIC: Intrinsic = flagged(rt::GUARD_FAILED_PANIC, &[W], DIVERGES);

impl Intrinsic {
    /// The C name the runtime library exports the entry point under.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// What the entry point does, as every stage reads it.
    pub fn effect(&self) -> RuntimeEffect {
        let mut effect = RuntimeEffect::new(self.params.to_vec());
        effect.result = self.result;
        effect.diverges = self.flags.diverges;
        effect.leaves_headers = self.flags.leaves_headers;
        effect.reads_count = self.flags.reads_count;
        effect.matches = self.flags.matches_receiver.then_some(0);
        effect
    }

    /// A callee naming the entry point, carrying its record.
    pub fn callee(&self) -> RuntimeCallee {
        RuntimeCallee::new(self.name, Rc::new(self.effect()))
    }
}

/// Every entry point only the compiler calls.
const INTRINSICS: &[&Intrinsic] = &[
    &LIST_COW,
    &MAP_COW,
    &SET_COW,
    &ARRAY_COW,
    &STRING_APPEND,
    &STRING_APPEND_INT,
    &STRING_APPEND_UINT,
    &MAP_GET_CHECKED,
    &LIST_NEW_FROM_RAW,
    &LIST_NEW_FROM_MANAGED_ARRAY,
    &ARRAY_SLICE,
    &FLOAT_HASH,
    &FLOAT_TO_STRING,
    &F32_TO_STRING,
    &INT_TO_STRING,
    &UINT_TO_STRING,
    &BOOL_TO_STRING,
    &I128_TO_STRING,
    &U128_TO_STRING,
    &FORMAT_FIXED,
    &FORMAT_PAD,
    &FORMAT_RADIX,
    &ASSERT_PANICS,
    &READBACK,
    &UPLOAD,
    &ACQUIRE,
    &TRANSFER,
    &RELEASE,
    &RECEIVER_UNIQUE,
    &RC_IS_SHARED,
    &RC_STATS_DUP,
    &RC_STATS_FREE,
    &RC_STATS_COW_COPY,
    &ASSERT_FAIL,
    &ASSERT_EQ_FAIL,
    &ASSERT_NE_FAIL,
    &METHOD_NOT_CHECKED_PANIC,
    &GUARD_FAILED_PANIC,
];

/// The copy-on-write entry point of the built-in collection `kind`.
pub fn copy_on_write_of(kind: BuiltinCollectionKind) -> &'static Intrinsic {
    match kind {
        BuiltinCollectionKind::List => &LIST_COW,
        BuiltinCollectionKind::Set => &SET_COW,
        BuiltinCollectionKind::Map => &MAP_COW,
        BuiltinCollectionKind::Array => &ARRAY_COW,
    }
}

/// The entry point only the compiler calls under `name`, if there is one.
pub fn compiler_intrinsic(name: &str) -> Option<&'static Intrinsic> {
    INTRINSICS.iter().copied().find(|entry| entry.name == name)
}

/// The name of every entry point only the compiler calls.
pub fn compiler_intrinsic_names() -> impl Iterator<Item = &'static str> {
    INTRINSICS.iter().map(|entry| entry.name)
}
