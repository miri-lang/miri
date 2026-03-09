// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::codegen::cranelift::CraneliftBackend;
use miri::codegen::Backend;

#[test]
fn test_cranelift_backend_new() {
    let backend = CraneliftBackend::new();
    assert!(backend.is_ok(), "Failed to create Cranelift backend");

    let backend = backend.unwrap();
    assert_eq!(backend.name(), "cranelift");
}
