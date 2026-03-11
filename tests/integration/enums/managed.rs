// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_managed_payload_drop() {
    assert_runs(
        r#"
use system.collections.list

enum Result
    Success(List<int>)
    Error(String)

fn main()
    // This creates an enum with a managed payload.
    // When `r` goes out of scope, the enum's drop code should decrement the payload's RC.
    // If it doesn't, this will leak memory (verified by leak sanitizer/Miri checks).
    let r = Result.Success(List([1, 2, 3]))
    
    let r2 = Result.Error("Something went wrong")
"#,
    );
}
