// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_option_wrapping_collection() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let opt List<int>? = Some(List([1, 2, 3]))
    
    match opt
        Some(l): println(f"{l.length()}")
        None: println("error")
            
    // Reassigning to None should drop the collection
    var opt2 List<int>? = Some(List([4, 5]))
    opt2 = None
    println("done")
"#,
        "3\ndone",
    );
}
