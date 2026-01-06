// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::StorageClass;

#[test]
fn test_lower_shared_variable() {
    let source = "
gpu fn kernel()
    shared cache [float; 256]
";
    let body = lower_code(source);

    // Check for local 'cache'
    let mut found_cache = false;
    for decl in &body.local_decls {
        if let Some(name) = &decl.name {
            if name == "cache" {
                found_cache = true;
                assert_eq!(
                    decl.storage_class,
                    StorageClass::GpuShared,
                    "Variable 'cache' should be shared"
                );
                break;
            }
        }
    }
    assert!(found_cache, "Did not find local variable 'cache'");
}
