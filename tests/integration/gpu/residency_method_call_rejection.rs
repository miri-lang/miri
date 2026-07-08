// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type-checker rejection of method calls on gpu-resident user-defined class types.
//!
//! A gpu-resident binding holds data in a device buffer, but method dispatch is a
//! host-context operation. Until cross-residency call analysis is implemented
//! (a future enhancement), method calls on gpu-resident receivers are rejected at
//! compile time to prevent UB (the receiver's bytes are device-resident, not host-addressable).
//!
//! This test suite includes:
//! 1. Negative tests: method calls on gpu-resident class bindings are rejected.
//! 2. Guard tests: legal residency-polymorphic methods on gpu-resident collections
//!    (e.g., `.length()` on a gpu-resident static Array) are NOT rejected.

use crate::integration::utils::{assert_compiler_error, assert_type_checks};

/// A method call on a gpu-resident class type is rejected at type-check time.
/// Uses a class method `magnitude()` which is definitely a method call.
#[test]
fn gpu_resident_class_method_call_rejected() {
    assert_compiler_error(
        "
use system.accelerator

class Vector implements Accelerable
    var x int
    var y int

    fn magnitude() int
        self.x + self.y

fn main()
    gpu var g = Vector(x: 3, y: 4)
    let m = g.magnitude()
",
        "cannot call method",
    );
}

/// The diagnostic mentions "gpu-resident" in the error message.
#[test]
fn gpu_resident_method_call_diagnostic_mentions_gpu_residency() {
    assert_compiler_error(
        "
use system.accelerator

class Point implements Accelerable
    var x int

    fn get_x() int
        self.x

fn main()
    gpu var my_point = Point(x: 10)
    let x = my_point.get_x()
",
        "gpu-resident",
    );
}

/// The diagnostic suggests the fix-it: copy to host first, then call the method.
#[test]
fn gpu_resident_method_call_diagnostic_suggests_host_copy() {
    assert_compiler_error(
        "
use system.accelerator

class Point implements Accelerable
    var x int

    fn get_x() int
        self.x

fn main()
    gpu var g = Point(x: 5)
    let x = g.get_x()
",
        "let h = g",
    );
}

/// Buffer-touching MIR-intercepted methods on gpu-resident static Arrays are STILL
/// allowed if they are in the fail-closed whitelist (e.g., .length() is safe because
/// it doesn't touch the device buffer, only reads the host-side length metadata).
/// This guard test ensures we don't over-reject.
#[test]
fn gpu_resident_array_length_method_still_allowed() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let a = Array<i32, 4>(1, 2, 3, 4)
    let len = a.length()
",
    );
}

/// Guard test: .slice() on a gpu-resident Array is allowed (performs readback).
#[test]
fn gpu_resident_array_slice_method_still_allowed() {
    assert_type_checks(
        "
use system.collections.array

fn main()
    gpu let a = Array<i32, 4>(1, 2, 3, 4)
    let slice_result = a.slice(0..2)
",
    );
}

/// Guard test: .reduce() on a gpu-resident Array is allowed (performs reduction).
#[test]
fn gpu_resident_array_reduce_method_still_allowed() {
    assert_type_checks(
        "
use system.collections.array

fn add(a int, b int) int
    a + b

fn main()
    gpu let a = Array<i32, 4>(1, 2, 3, 4)
    let sum = a.reduce(0, add)
",
    );
}

/// A method call on a HOST-resident (non-gpu) class must still compile normally.
#[test]
fn host_resident_class_method_call_still_allowed() {
    assert_type_checks(
        "
class Point
    var x int
    var y int

fn main()
    let g = Point(x: 3, y: 4)
    let x = g.x
",
    );
}
