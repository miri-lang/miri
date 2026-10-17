// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `MIRI_RC_STATS=1` reports what a program's reference counting did: the
//! increments and decrements compiled code made, the decrements that released
//! a value, the copy-on-write copies and the buffers the runtime grew.

use crate::utils::miri_run_with_env_multiple;

/// The report line `MIRI_RC_STATS` printed.
fn rc_stats(code: &str, extra: &[(&str, &str)]) -> String {
    let mut env = vec![("MIRI_RC_STATS", "1")];
    env.extend_from_slice(extra);
    let result = miri_run_with_env_multiple(code, &env);
    assert!(result.success, "the program failed:\n{}", result.output());
    result
        .stderr
        .lines()
        .find(|line| line.starts_with("MIRI_RC_STATS:"))
        .unwrap_or_else(|| panic!("no MIRI_RC_STATS line in:\n{}", result.stderr))
        .to_string()
}

/// `main` makes one increment (the second binding of the box) and three
/// decrements (the box's two bindings and the formatted string), two of which
/// release a value: the box and the string.
const SHARED_BOX: &str = r#"
use system.io

class Box
    var value int

    fn init(value int)
        self.value = value

fn read(b Box) int: b.value

let a = Box(5)
let b = a
println(f"{read(a) + read(b)}")
"#;

#[test]
fn test_rc_stats_counts_exactly_the_operations_compiled_code_makes() {
    assert_eq!(
        rc_stats(SHARED_BOX, &[]),
        "MIRI_RC_STATS: dup=1 drop=3 drop_free=2 cow_copy=0 realloc=0"
    );
}

#[test]
fn test_rc_stats_counts_a_copy_on_write_copy() {
    let line = rc_stats(
        r#"
use system.io
use system.collections.list

var a = List<int>()
a.push(1)
var b = a
b.push(2)
println(f"{a.length()} {b.length()}")
"#,
        &[],
    );
    assert!(line.contains("cow_copy=1"), "{line}");
}

/// A write through `b.items` finds both the struct `b` and its list shared
/// with `a`, and copies each — two copies, counted although no copy-on-write
/// entry point made them. A second write, through what is now `b`'s own,
/// copies nothing more.
#[test]
fn test_rc_stats_counts_the_copies_a_write_through_a_path_makes() {
    let program = |writes: &str| {
        format!(
            "use system.io\nuse system.collections.list\n\nstruct Bag\n    items [int]\n\nvar a = Bag(items: List([1, 2]))\nvar b = a\n{writes}println(f\"{{a.items[0]}} {{b.items[0]}}\")\n"
        )
    };
    let once = rc_stats(&program("b.items[0] = 5\n"), &[]);
    let twice = rc_stats(&program("b.items[0] = 5\nb.items[1] = 6\n"), &[]);
    assert!(once.contains("cow_copy=2"), "{once}");
    assert!(twice.contains("cow_copy=2"), "{twice}");
}

/// A write into a list nothing else holds copies nothing.
#[test]
fn test_rc_stats_counts_no_copy_for_an_unshared_write_through_a_path() {
    let line = rc_stats(
        r#"
use system.io
use system.collections.list

struct Cell
    x int

var a = List([Cell(x: 1)])
a[0].x = 2
println(f"{a[0].x}")
"#,
        &[],
    );
    assert!(line.contains("cow_copy=0"), "{line}");
}

/// A struct rebuilt because the list holding it was shared counts as a copy
/// as well: the write copies the shared list, the two lists then share the
/// element, and the write rebuilds it — two copies.
#[test]
fn test_rc_stats_counts_a_struct_copied_by_a_write_through_a_path() {
    let line = rc_stats(
        r#"
use system.io
use system.collections.list

struct Cell
    x int
    name String

var a = List([Cell(x: 1, name: "n")])
var b = a
b[0].x = 2
println(f"{a[0].x} {b[0].x}")
"#,
        &[],
    );
    assert!(line.contains("cow_copy=2"), "{line}");
}

#[test]
fn test_rc_stats_counts_the_buffers_the_runtime_grew() {
    let line = rc_stats(
        r#"
use system.io
use system.collections.list

var xs = List<int>()
var i = 0
while i < 1000
    xs.push(i)
    i += 1
println(f"{xs.length()}")
"#,
        &[],
    );
    let realloc: usize = line
        .split("realloc=")
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(
        realloc >= 5,
        "growing a list to 1000 elements reallocates its buffer: {line}"
    );
}

#[test]
fn test_rc_stats_runs_beside_the_leak_check_and_the_heap_guard() {
    let line = rc_stats(
        SHARED_BOX,
        &[("MIRI_LEAK_CHECK", "1"), ("MIRI_HEAP_GUARD", "1")],
    );
    assert_eq!(
        line,
        "MIRI_RC_STATS: dup=1 drop=3 drop_free=2 cow_copy=0 realloc=0"
    );
}

#[test]
fn test_without_the_flag_nothing_is_reported() {
    let result = miri_run_with_env_multiple(SHARED_BOX, &[]);
    assert!(result.success);
    assert!(
        !result.stderr.contains("MIRI_RC_STATS"),
        "{}",
        result.stderr
    );
}
