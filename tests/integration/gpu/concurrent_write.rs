// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Concurrent-write check for `forall` kernel bodies (syntactic baseline).
//!
//! A write `arr[i] = e` to a non-atomic gpu buffer inside a `forall` pass is
//! safe only when the index `i` is provably unique per thread — a literal, the
//! `forall` variable, or a linear function of it. A data-dependent index
//! (a buffer read used as a subscript, or an integer division/modulo that folds
//! distinct threads onto one element) races and is rejected at compile time.
//! The escape hatch is an atomic element (`Array<Atomic<T>, N>`).

use super::helpers::assert_gpu_wgsl_valid;
use crate::integration::utils::assert_compiler_error;

/// The canonical safe write: index by the `forall` variable directly.
#[test]
fn write_indexed_by_forall_variable_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i
",
    );
}

/// A linear function of the `forall` variable stays 1-1, so it is allowed.
#[test]
fn write_indexed_by_linear_function_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 8>()
    gpu forall i in 0..4
        out[i * 2 + 1] = i
",
    );
}

/// The standard 2D flatten `y * W + x` over two `forall` variables is affine
/// in both and stays 1-1, so it is allowed.
#[test]
fn write_indexed_by_flattened_2d_index_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var dst = Array<int, 12>()
    gpu forall x, y in 0..4, 0..3
        dst[y * 4 + x] = x + y
",
    );
}

/// A scatter — the write index is itself a buffer read — is not provably 1-1
/// and is rejected.
#[test]
fn write_with_buffer_read_index_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu let perm = [3, 2, 1, 0]
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[perm[i]] = i
",
        "not provably unique per thread",
    );
}

/// A write index taken from a local that was itself read out of a buffer is a
/// disguised scatter and is rejected.
#[test]
fn write_indexed_by_buffer_derived_local_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu let src = [0, 0, 1, 1]
    gpu var hist = Array<int, 4>()
    gpu forall i in 0..4
        let bin = src[i]
        hist[bin] = 1
",
        "not provably unique per thread",
    );
}

/// An integer-division index folds distinct threads onto one element, so it is
/// not 1-1 and is rejected.
#[test]
fn write_with_divided_index_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i / 2] = i
",
        "not provably unique per thread",
    );
}

/// A modulo index folds distinct threads onto one element (wrap-around), so it
/// is not 1-1 and is rejected.
#[test]
fn write_with_modulo_index_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..8
        out[i % 4] = i
",
        "not provably unique per thread",
    );
}

/// A non-injective write nested under an `if` is still reached by the walk and
/// rejected.
#[test]
fn write_under_conditional_is_checked() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu let perm = [3, 2, 1, 0]
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        if i > 0
            out[perm[i]] = i
",
        "not provably unique per thread",
    );
}

/// The atomic element type is the sanctioned escape hatch: a data-dependent
/// index into an `Atomic` buffer is race-free by construction and allowed.
#[test]
fn data_dependent_index_into_atomic_buffer_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var hist = Array<Atomic<u32>, 4>()
    gpu forall i in 0..4
        atomic_add(hist, i % 4, 1 as u32)
",
    );
}
