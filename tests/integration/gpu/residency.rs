// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Residency-keyword surface: `gpu let` / `gpu var` parse, type-check, and
// stamp `BindingResidency::Gpu` on the local. Mixed-residency operators
// produce the cross-residency arithmetic diagnostic.

use super::utils::*;

#[test]
fn gpu_let_with_array_literal_type_checks() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let g = [1, 2, 3]
",
    );
}

#[test]
fn gpu_var_scalar_type_checks() {
    assert_type_checks(
        "
fn main()
    gpu var g = 0
",
    );
}

#[test]
fn mixed_residency_addition_is_rejected() {
    assert_compiler_error(
        "
fn main()
    var g_host = 0
    gpu var g = 0
    let s = g_host + g
",
        "cannot add gpu-resident 'g' and host-resident 'g_host'; \
         bring both to the same residency first.",
    );
}

#[test]
fn mixed_residency_subtraction_is_rejected() {
    assert_compiler_error(
        "
fn main()
    var g_host = 0
    gpu var g = 0
    let s = g_host - g
",
        "cannot subtract gpu-resident 'g' and host-resident 'g_host'; \
         bring both to the same residency first.",
    );
}

#[test]
fn mixed_residency_multiplication_is_rejected() {
    assert_compiler_error(
        "
fn main()
    var g_host = 0
    gpu var g = 0
    let s = g_host * g
",
        "cannot multiply gpu-resident 'g' and host-resident 'g_host'; \
         bring both to the same residency first.",
    );
}

#[test]
fn same_residency_addition_is_accepted() {
    assert_type_checks(
        "
fn main()
    gpu var a = 0
    gpu var b = 0
    let s = a + b
",
    );
}
