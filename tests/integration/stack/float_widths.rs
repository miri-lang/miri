// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Stack<T> at non-int element widths.
//!
//! Blocked on builtin-collection method lowering: `List<T>` is a `TypeKind::List`,
//! so it never enters generic-class monomorphization and its Miri-defined methods
//! (`remove_at`, `pop`) lower once at pointer width.

use crate::integration::utils::*;

#[test]
#[ignore = "List<T> methods written in Miri (remove_at/pop) lower once at pointer width: 'List_remove_at/List_pop ... returns [F64] is incompatible with previous declaration ... returns [I64]'. Builtin collections are TypeKind::List, so they never enter generic-class monomorphization."]
fn test_stack_float_push_pop() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    var s = Stack<float>()
    s.push(1.5)
    s.push(2.5)
    s.push(3.5)
    println(f"{s.pop() ?? 0.0}")
    println(f"{s.pop() ?? 0.0}")
    println(f"{s.pop() ?? 0.0}")
"#,
        "3.5
2.5
1.5",
    );
}
