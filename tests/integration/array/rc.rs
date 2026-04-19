// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Array<List<T>> elem_drop_fn (task 2.4a) ──────────────────────────────────

#[test]
fn test_array_of_lists_no_crash() {
    // Array<List<int>>: elem_drop_fn must be set so that miri_rt_array_free
    // DecRefs each inner list.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = [List([1, 2]), List([3, 4])]
    println(f"{a[0][0]}")
"#,
        "1",
    );
}

#[test]
fn test_array_of_lists_reassign_no_crash() {
    // Reassigning an Array<List<int>> must free the old array and DecRef each
    // inner list.  Without elem_drop_fn the old inner lists would leak; with a
    // buggy double-free they would crash.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var a = [List([1, 2]), List([3, 4])]
    a = [List([5, 6]), List([7, 8])]
    println(f"{a[0][0]}")
"#,
        "5",
    );
}

#[test]
fn test_list_of_lists_no_double_free() {
    // List([List([...])]) — the temp array inside the List constructor must NOT
    // cause a double-free.  LIST_NEW_FROM_MANAGED_ARRAY IncRefs each element;
    // the temp array's elem_drop_fn provides the matching DecRef.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let outer = List([List([1, 2, 3])])
    println(f"{outer[0][0]}")
"#,
        "1",
    );
}

#[test]
fn test_list_of_lists_reassign_no_crash() {
    // Reassign a List<List<int>>; old inner list must be properly DecRef'd.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var outer = List([List([1, 2])])
    outer = List([List([3, 4])])
    println(f"{outer[0][1]}")
"#,
        "4",
    );
}

// ── Array<String> scope-exit cleanup (task 2.5) ──────────────────────────────

#[test]
fn test_array_of_strings_out_of_scope_no_crash() {
    // Array<String> going out of scope inside a function must DecRef all string
    // elements via the inline elem-drop loop.  If the loop is missing, the
    // strings would leak; if it fires twice, they would double-free and crash.
    assert_runs(
        r#"
use system.collections.array

fn make_strings()
    let a = ["one", "two", "three"]
    // a goes out of scope here — strings must be DecRef'd

fn main()
    make_strings()
    make_strings()
"#,
    );
}

// ── Drop-fn setter wiring (task 2.4) ─────────────────────────────────────────

#[test]
fn test_array_of_strings_no_crash() {
    // Array<String>: elem_drop_fn must be registered so that miri_rt_array_free
    // DecRefs each string element. Verifies the setter call is emitted.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a = ["hello", "world"]
    println(f"{a[0]}")
"#,
        "hello",
    );
}

#[test]
fn test_array_of_strings_reassign_no_crash() {
    // Reassigning an Array<String> must free the old array and its string elements.
    // Both arrays must have the same size (arrays are fixed-size in Miri).
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var a = ["alpha", "beta"]
    a = ["gamma", "delta"]
    println(f"{a[0]}")
"#,
        "gamma",
    );
}

#[test]
fn test_array_of_nonimmmortal_strings_no_double_free() {
    // Array<String> with dynamically-created (non-immortal) strings must not
    // double-free: the inline elem-drop loop in emit_type_drop is sufficient;
    // setting elem_drop_fn on top would cause use-after-free when RC hits zero.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let s = "hel" + "lo"
    let arr = [s, "world"]
    println(f"{arr[0]}")
"#,
        "hello",
    );
}

#[test]
fn test_array_alias_no_double_free() {
    // Two variables pointing at the same array should not double-free.
    // RC is incremented on alias, decremented when each goes out of scope.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var a = [1, 2, 3]
    var b = a
    println(f"{b[0]}")
    "#,
        "1",
    );
}

#[test]
fn test_array_reassign_frees_old() {
    // Reassigning a collection variable should free the old value via DecRef.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var a = [1, 2, 3]
    a = [4, 5, 6]
    println(f"{a[0]}")
    "#,
        "4",
    );
}

#[test]
fn test_array_passed_to_function_no_dangle() {
    // A collection passed to a function should not dangle after return.
    assert_runs_with_output(
        r#"
use system.io

fn sum_first(arr [int; 3]) int
    arr[0] + arr[1] + arr[2]

fn main()
    let a = [10, 20, 30]
    let s = sum_first(arr: a)
    println(f"{s}")
    "#,
        "60",
    );
}

// ── Array.set incref for managed elements (task 3.1) ─────────────────────────

#[test]
fn test_array_set_managed_val_incref() {
    // array.set with a managed String variable; after the local goes out of scope
    // the array must still hold a valid reference (IncRef'd at set time).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn set_string() [String; 2]
    let s = "h" + "i"
    var a = ["", ""]
    a.set(0, s)
    return a
    // s goes out of scope — array must still own "hi"

fn main()
    let a = set_string()
    println(a[0])
"#,
        "hi",
    );
}

// ── Direct index write RC correctness ────────────────────────────────────────

#[test]
fn test_array_index_write_managed_no_leak() {
    // a[i] = new_val (direct index write syntax) must use Copy semantics so that
    // Perceus IncRefs the source before storing.  Without this the statement-level
    // result-temp DecRef would free the just-stored allocation, leaving a dangling
    // pointer in the slot.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    var a = ["seed"]
    var i = 0
    while i < 100
        a[0] = "x" + "y"
        i = i + 1
    println(a[0])
"#,
        "xy",
    );
}

// ── Set/overwrite decref old value (task 3.2) ────────────────────────────────

#[test]
fn test_array_set_overwrite_managed_no_leak() {
    // array.set(i, new_val) must DecRef the old managed element. Overwriting the
    // same slot 100 times with fresh concat strings would exhaust memory / crash
    // if the old RC is never decremented.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array

fn main()
    var a = ["seed"]
    var i = 0
    while i < 100
        a.set(0, "x" + "y")
        i = i + 1
    println(a[0])
"#,
        "xy",
    );
}
