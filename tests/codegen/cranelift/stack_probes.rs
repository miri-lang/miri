// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function whose frame is larger than a guard page touches its frame a page
//! at a time while allocating it. Without the probes the prologue moves the
//! stack pointer down by the whole frame and stores at the new bottom first,
//! so a deep recursion through such a frame writes past the guard page below
//! the stack instead of faulting on it.

use cranelift_codegen::settings::ProbestackStrategy;
use miri::codegen::cranelift::{isa_flags, CraneliftOptions, OptLevel};

/// The guard region the probes assume, as a log2 of its size: one 4 KiB page,
/// the smallest any supported target maps below a stack.
const GUARD_PAGE_LOG2: u8 = 12;

fn every_build() -> Vec<(&'static str, CraneliftOptions)> {
    vec![
        ("debug", CraneliftOptions::default()),
        (
            "release",
            CraneliftOptions {
                opt_level: OptLevel::Speed,
                runtime_checks: false,
                ..CraneliftOptions::default()
            },
        ),
        (
            "profile",
            CraneliftOptions {
                opt_level: OptLevel::Speed,
                preserve_frame_pointers: true,
                ..CraneliftOptions::default()
            },
        ),
    ]
}

#[test]
fn every_build_probes_a_frame_larger_than_a_guard_page() {
    for (build, options) in every_build() {
        let flags = isa_flags(&options).expect("the flags should build");
        assert!(flags.enable_probestack(), "{build}: stack probes are off");
        assert_eq!(
            flags.probestack_strategy(),
            ProbestackStrategy::Inline,
            "{build}: probes must be inline, since no runtime defines a probe function"
        );
        assert_eq!(
            flags.probestack_size_log2(),
            GUARD_PAGE_LOG2,
            "{build}: probes must step by one guard page"
        );
    }
}
