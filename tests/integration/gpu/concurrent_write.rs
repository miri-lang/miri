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

/// A method-form write `buf.set(index, value)` is checked exactly like a
/// subscript write: a buffer-derived index is a disguised scatter and rejected.
#[test]
fn method_set_at_buffer_derived_index_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu let perm = [3, 2, 1, 0]
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out.set(perm[i], i)
",
        "not provably unique per thread",
    );
}

/// A nested CPU loop iterates its full range in every `forall` thread, so a
/// write indexed by the nested-loop variable is written by every thread and
/// races. It is rejected.
#[test]
fn write_indexed_by_nested_loop_variable_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        for j in 0..4
            out[j] = i
",
        "not provably unique per thread",
    );
}

/// An affine index that mixes the `forall` variable with a nested-loop variable
/// collides across threads (thread `i`, iteration `j` and thread `i+1`,
/// iteration `j-1` hit the same element), so it too is rejected.
#[test]
fn write_indexed_by_forall_plus_nested_loop_variable_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 8>()
    gpu forall i in 0..4
        for j in 0..4
            out[i + j] = i
",
        "not provably unique per thread",
    );
}

/// Writing the `forall` element inside a nested loop targets exactly one element
/// per thread (re-written sequentially by the same thread), so it is allowed.
#[test]
fn write_forall_variable_inside_nested_loop_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        for j in 0..4
            out[i] = j
",
    );
}

/// A `gpu fn` explicit-launch kernel whose write is indexed by the per-thread
/// `kernel.global_idx` is unique per thread and allowed.
#[test]
fn kernel_write_indexed_by_global_idx_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    let i = kernel.global_idx.x
    dst[i] = i

fn main()
    gpu var out = Array<int, 4>()
    fill(out).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
",
    );
}

/// A `gpu fn` scatter — the write index is read out of another buffer — is not
/// provably unique per thread and is rejected at the kernel declaration.
#[test]
fn kernel_scatter_write_is_rejected() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

gpu fn scatter(dst out Array<int, 4>, idx Array<int, 4>)
    let i = kernel.global_idx.x
    dst[idx[i]] = i
",
        "not provably unique per thread",
    );
}

/// A `gpu fn` write whose index divides the thread id folds distinct threads
/// onto one element and is rejected.
#[test]
fn kernel_divided_index_write_is_rejected() {
    assert_compiler_error(
        "
use system.gpu
use system.collections.array

gpu fn fold(dst out Array<int, 4>)
    let i = kernel.global_idx.x
    dst[i / 2] = i
",
        "not provably unique per thread",
    );
}
