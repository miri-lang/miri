// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Compile-time validation of user `gpu fn` names against WGSL reserved forms.
//!
//! A `gpu fn` name is emitted verbatim as the WGSL entry-point / helper name.
//! WGSL reserves identifiers that begin with a double underscore and a fixed
//! set of keywords/reserved words, so a colliding name would otherwise fail
//! late during shader-module compilation with a generic backend error. The
//! type checker rejects such names up front with a source-cited diagnostic and
//! a rename hint.

use super::helpers::assert_gpu_wgsl_valid;
use crate::integration::utils::{assert_compiler_error, assert_runs_with_output};

/// A `gpu fn` whose name begins with the reserved `__` prefix is rejected at
/// compile time (not at shader-launch time) with a clear diagnostic.
#[test]
fn gpu_fn_double_underscore_name_rejected() {
    assert_compiler_error(
        "
use system.collections.array

gpu fn __k(a Array<f32,4>)
    let x = 1

fn main()
    let y = 1
",
        "reserved",
    );
}

/// A `gpu fn` whose name is a WGSL reserved word is rejected at compile time.
/// `virtual` is a valid Miri identifier but a reserved word in WGSL.
#[test]
fn gpu_fn_reserved_keyword_name_rejected() {
    assert_compiler_error(
        "
use system.collections.array

gpu fn virtual(a Array<f32,4>)
    let x = 1

fn main()
    let y = 1
",
        "reserved",
    );
}

/// The diagnostic names the offending function so the fix-it is actionable.
#[test]
fn gpu_fn_reserved_name_diagnostic_names_the_function() {
    assert_compiler_error(
        "
use system.collections.array

gpu fn __compute(a Array<f32,4>)
    let x = 1

fn main()
    let y = 1
",
        "__compute",
    );
}

/// The reserved-name check is scoped to `gpu fn`: a plain CPU `fn` whose name
/// begins with `__` is a valid host identifier and must not be rejected.
#[test]
fn plain_fn_double_underscore_name_accepted() {
    assert_runs_with_output(
        "
fn __helper(x int) int: x + 1

fn main()
    let y = __helper(41)
    println(f'{y}')
",
        "42",
    );
}

/// A `gpu fn` with an ordinary name is unaffected: the WGSL still compiles.
#[test]
fn gpu_fn_ordinary_name_accepted() {
    assert_gpu_wgsl_valid(
        "
use system.collections.array

gpu fn compute_tiles(a Array<f32,4>, b out Array<f32,4>)
    let x = 1

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var b = Array<f32,4>()
    compute_tiles(a, b).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
    );
}
