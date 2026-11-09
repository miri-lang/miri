// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bits of the GPU fault word: what a kernel records when it does
//! something the host would stop the program for.
//!
//! A kernel that can fault declares a module-scope `miri_fault: atomic<u32>`
//! and ORs the bit of each kind of fault it meets into it. The word is sticky:
//! every kind that fired since the host last looked stays set, so the host
//! reports them all at once. The compiler writes each bit into the kernels it
//! emits; the core runtime reads the word back into these kinds and reports
//! each under the code and sentence its own trap for that kind uses, which
//! this table names. The two do not link each other, so this file is compiled
//! into both: the compiler as `crate::abi::gpu_fault`, the core runtime
//! through a `#[path]` module. It depends on nothing but `core`.
//!
//! The browser runtime, `assets/web/miri-gpu.js`, cannot read Rust, so it
//! keeps its own copy of the table; a test compares that copy, and the report
//! it builds, with this file.

/// One kind of fault a kernel records in the fault word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GpuFault {
    /// An integer `+`, `-`, `*`, negation, `abs`, or signed `MIN / -1` or
    /// `MIN % -1`, whose result does not fit its type.
    IntegerOverflow,
    /// An integer `/` whose divisor is zero.
    DivisionByZero,
    /// An integer `%` whose divisor is zero.
    RemainderByZero,
    /// An index outside the collection it is applied to.
    IndexOutOfBounds,
}

impl GpuFault {
    /// Every kind, in the order of its bit, lowest first.
    pub const ALL: [GpuFault; 4] = [
        GpuFault::IntegerOverflow,
        GpuFault::DivisionByZero,
        GpuFault::RemainderByZero,
        GpuFault::IndexOutOfBounds,
    ];

    /// The bit of the fault word that records this kind.
    pub const fn bit(self) -> u32 {
        match self {
            GpuFault::IntegerOverflow => 1 << 0,
            GpuFault::DivisionByZero => 1 << 1,
            GpuFault::RemainderByZero => 1 << 2,
            GpuFault::IndexOutOfBounds => 1 << 3,
        }
    }

    /// The runtime code the host's trap for this kind reports.
    pub const fn code(self) -> &'static str {
        match self {
            GpuFault::IntegerOverflow => "MER_RT_003",
            GpuFault::DivisionByZero => "MER_RT_001",
            GpuFault::RemainderByZero => "MER_RT_002",
            GpuFault::IndexOutOfBounds => "MER_RT_011",
        }
    }

    /// The sentence the host's trap for this kind reports. An index is
    /// reported without its value or the collection's length, which a kernel
    /// does not record.
    pub const fn sentence(self) -> &'static str {
        match self {
            GpuFault::IntegerOverflow => "integer overflow: the result does not fit its type",
            GpuFault::DivisionByZero => "division by zero",
            GpuFault::RemainderByZero => "remainder by zero",
            GpuFault::IndexOutOfBounds => "index out of bounds",
        }
    }
}

/// What a report of a non-zero fault word begins with.
pub const SUMMARY_PREFIX: &str = "a GPU kernel faulted: ";

/// What separates two kinds in a report.
pub const SUMMARY_SEPARATOR: &str = "; ";

/// Writes the report of every kind `word` records, lowest bit first, each as
/// `sentence (code)`: `a GPU kernel faulted: division by zero (MER_RT_001);
/// index out of bounds (MER_RT_011)`. Writes nothing and returns `Ok(false)`
/// when `word` records no kind this table knows.
pub fn write_summary(word: u32, out: &mut impl core::fmt::Write) -> Result<bool, core::fmt::Error> {
    let mut written = false;
    for fault in recorded_faults(word) {
        out.write_str(if written {
            SUMMARY_SEPARATOR
        } else {
            SUMMARY_PREFIX
        })?;
        write!(out, "{} ({})", fault.sentence(), fault.code())?;
        written = true;
    }
    Ok(written)
}

/// The kinds `word` records, lowest bit first.
pub fn recorded_faults(word: u32) -> impl Iterator<Item = GpuFault> {
    GpuFault::ALL
        .into_iter()
        .filter(move |fault| word & fault.bit() != 0)
}
