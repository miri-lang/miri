// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `gpu forall` capture residency rules. A `gpu forall` body may only capture
// identifiers whose binding residency is `Gpu`. Capturing a host-resident
// buffer is a type error with dual machine-applicable fix-its. No implicit
// uploads — residency is source-visible.

use super::utils::*;

#[test]
fn host_resident_buffer_capture_is_rejected() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3]
    gpu forall i in 0..3
        a[i] = i
",
        "'gpu forall' capture 'a' must be gpu-resident",
    );
}

#[test]
fn host_capture_diagnostic_proposes_annotation_fixit() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3]
    gpu forall i in 0..3
        a[i] = i
",
        "Annotate the binding with 'gpu let'",
    );
}

#[test]
fn host_capture_diagnostic_proposes_explicit_copy_fixit() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3]
    gpu forall i in 0..3
        a[i] = i
",
        "gpu let a_gpu = a",
    );
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_resident_buffer_capture_compiles_and_runs() {
    assert_runs(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var a = [0, 0, 0]
    gpu forall i in 0..3
        a[i] = i
",
    );
}

#[test]
fn read_only_host_capture_is_also_rejected() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

fn main()
    let src = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = src[i] * 2
",
        "'gpu forall' capture 'src' must be gpu-resident",
    );
}
