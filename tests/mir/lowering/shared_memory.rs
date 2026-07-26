// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{local_decl, mir_lower_code};
use miri::mir::StorageClass;

#[test]
fn test_lower_shared_variable() {
    let body = mir_lower_code(
        "
gpu fn kernel()
    shared cache [float; 256]
",
    );
    let decl = local_decl(&body, "cache").expect("Expected local 'cache'");
    assert_eq!(decl.storage_class, StorageClass::GpuShared);
}
