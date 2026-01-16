// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lowering_storage_class_test;
use miri::mir::StorageClass;

#[test]
fn test_lower_shared_variable() {
    mir_lowering_storage_class_test(
        "
gpu fn kernel()
    shared cache [float; 256]
",
        "cache",
        StorageClass::GpuShared,
    );
}
