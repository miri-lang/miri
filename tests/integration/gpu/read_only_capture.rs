// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for F3 (read-only vs read-write GPU captures).
//!
//! A capture that is never written to in the kernel body should emit
//! `var<storage, read>` in the WGSL shader, while a written capture
//! should emit `var<storage, read_write>`.

use super::helpers::compile_to_wgsl;

#[test]
fn read_only_capture_emits_read_storage_qualifier() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let src = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = src[i] * 2
",
    );

    // `src` is read-only (never assigned to), so should emit `var<storage, read>`.
    assert!(
        wgsl.contains("var<storage, read>"),
        "Expected read-only storage binding in WGSL:\n{}",
        wgsl
    );
}

#[test]
fn written_capture_emits_read_write_storage_qualifier() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let src = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = src[i] * 2
",
    );

    // `dst` is read-write (assigned to), so should emit `var<storage, read_write>`.
    assert!(
        wgsl.contains("var<storage, read_write>"),
        "Expected read-write storage binding in WGSL:\n{}",
        wgsl
    );
}

#[test]
fn mixed_read_only_and_read_write_captures() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var result = [0, 0, 0, 0]
    gpu for i in 0..4
        result[i] = a[i] + b[i]
",
    );

    // Both `a` and `b` should be read-only.
    let read_count = wgsl.matches("var<storage, read>").count();
    assert!(
        read_count >= 2,
        "Expected at least 2 read-only bindings (a and b), found {}. WGSL:\n{}",
        read_count,
        wgsl
    );

    // `result` should be read-write.
    assert!(
        wgsl.contains("var<storage, read_write>"),
        "Expected read-write storage binding for result. WGSL:\n{}",
        wgsl
    );
}

#[test]
fn capture_written_in_index_assignment_is_read_write() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let src = [1, 2, 3, 4]
    gpu var buf = [0, 0, 0, 0]
    gpu for i in 0..4
        buf[i] = src[i]
",
    );

    // `buf` is written via index assignment, so should be read_write.
    assert!(
        wgsl.contains("var<storage, read_write>"),
        "Expected read-write storage binding. WGSL:\n{}",
        wgsl
    );
}

#[test]
fn capture_only_read_is_read_only() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let data = [1, 2, 3, 4]
    gpu var sum = [0, 0, 0, 0]
    gpu for i in 0..4
        sum[i] = data[i]
",
    );

    // `data` is only read, never written, so should be read-only.
    // `sum` is written, so should be read_write.
    // Verify both qualifiers appear.
    assert!(
        wgsl.contains("var<storage, read>"),
        "Expected at least one read-only binding. WGSL:\n{}",
        wgsl
    );
    assert!(
        wgsl.contains("var<storage, read_write>"),
        "Expected at least one read-write binding. WGSL:\n{}",
        wgsl
    );
}
