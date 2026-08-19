// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Queue<T> at non-int element widths.
//!
//! Blocked on builtin-collection method lowering: `List<T>` is a `TypeKind::List`,
//! so it never enters generic-class monomorphization and its Miri-defined methods
//! (`remove_at`, `pop`) lower once at pointer width.

use crate::integration::utils::*;

#[test]
#[ignore = "List<T> methods written in Miri (remove_at/pop) lower once at pointer width: 'List_remove_at/List_pop ... returns [F64] is incompatible with previous declaration ... returns [I64]'. Builtin collections are TypeKind::List, so they never enter generic-class monomorphization."]
fn test_queue_float_enqueue_dequeue() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    var q = Queue<float>()
    q.enqueue(1.5)
    q.enqueue(2.5)
    q.enqueue(3.5)
    println(f"{q.dequeue() ?? 0.0}")
    println(f"{q.dequeue() ?? 0.0}")
    println(f"{q.dequeue() ?? 0.0}")
"#,
        "1.5
2.5
3.5",
    );
}

#[test]
#[ignore = "List<T> methods written in Miri (remove_at/pop) lower once at pointer width: 'List_remove_at/List_pop ... returns [F64] is incompatible with previous declaration ... returns [I64]'. Builtin collections are TypeKind::List, so they never enter generic-class monomorphization."]
fn test_queue_float_mixed_operations() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    var q = Queue<float>()
    q.enqueue(5.5)
    println(f"{q.dequeue() ?? 0.0}")
    q.enqueue(10.5)
    q.enqueue(15.5)
    println(f"{q.dequeue() ?? 0.0}")
"#,
        "5.5
10.5",
    );
}
