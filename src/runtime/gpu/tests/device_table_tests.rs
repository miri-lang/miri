// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::device_table::*;

#[test]
fn release_of_absent_handle_is_a_noop() {
    assert!(!release(u64::MAX));
}
