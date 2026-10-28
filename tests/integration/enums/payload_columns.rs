// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Each payload position of an enum value is one column shared by every
//! variant, as wide as the widest field any variant stores there. A wide
//! payload widens only its own position, so these tests mix widths per
//! position — a string beside a 128-bit integer, a float beside a byte, a
//! struct held as its bytes beside a string — and check that every field is
//! read back, copied and released where it was written.

use super::utils::*;

const MIXED: &str = r#"
use system.io
use system.collections.list

struct Block
    a i128
    b i128
    c i128
    d int

enum Mixed
    Named(String, u128, f32)
    Held(Block, String)
    Small(u8, float, String)
    Bare

fn show(m Mixed)
    match m
        Mixed.Named(s, n, f): println(f"named {s} {n} {f}")
        Mixed.Held(b, s): println(f"held {b.a} {b.b} {b.c} {b.d} {s}")
        Mixed.Small(x, f, s): println(f"small {x} {f} {s}")
        Mixed.Bare: println("bare")
"#;

fn with_mixed(main: &str) -> String {
    format!("{MIXED}\n{main}")
}

#[test]
fn test_enum_columns_read_back_every_variant() {
    assert_heap_guard_output(
        &with_mixed(
            r#"
fn main()
    show(Mixed.Named("n" + "1", 340282366920938463463374607431768211455, 1.5))
    show(Mixed.Held(Block(a: 36893488147419103274, b: -36893488147419103275, c: 7, d: 9), "h" + "2"))
    show(Mixed.Small(200, 2.25, "s" + "3"))
    show(Mixed.Bare)
"#,
        ),
        "named n1 340282366920938463463374607431768211455 1.5\n\
         held 36893488147419103274 -36893488147419103275 7 9 h2\n\
         small 200 2.25 s3\n\
         bare",
    );
}

#[test]
fn test_enum_columns_survive_a_copy_and_a_list() {
    assert_heap_guard_output(
        &with_mixed(
            r#"
fn main()
    let held = Mixed.Held(Block(a: 1, b: 36893488147419103274, c: -3, d: 4), "kept" + "")
    let copy = held
    let xs = List([Mixed.Small(7, 0.5, "x" + "y"), copy, Mixed.Named("z" + "", 18446744073709551617, 0.25)])
    for m in xs
        show(m)
    show(held)
"#,
        ),
        "small 7 0.5 xy\n\
         held 1 36893488147419103274 -3 4 kept\n\
         named z 18446744073709551617 0.25\n\
         held 1 36893488147419103274 -3 4 kept",
    );
}

const SLOT: &str = r#"
use system.io

struct Block
    a i128
    b i128
    c i128
    d int

enum Slot<T>
    Filled(T, String)
    Count(int, float)
"#;

#[test]
fn test_generic_enum_columns_at_a_wide_struct_and_at_a_string() {
    assert_heap_guard_output(
        &format!(
            "{SLOT}\n{}",
            r#"
fn show_block(s Slot<Block>)
    match s
        Slot.Filled(b, tag): println(f"block {b.a} {b.b} {b.c} {b.d} {tag}")
        Slot.Count(n, f): println(f"count {n} {f}")

fn show_string(s Slot<String>)
    match s
        Slot.Filled(v, tag): println(f"string {v} {tag}")
        Slot.Count(n, f): println(f"count {n} {f}")

fn main()
    let wide Slot<Block> = Slot.Filled(Block(a: 36893488147419103274, b: 2, c: -36893488147419103274, d: 5), "w" + "1")
    let narrow Slot<String> = Slot.Filled("v" + "1", "w" + "2")
    let counted Slot<Block> = Slot.Count(3, 1.25)
    let again = wide
    show_block(wide)
    show_string(narrow)
    show_block(counted)
    show_block(again)
"#
        ),
        "block 36893488147419103274 2 -36893488147419103274 5 w1\n\
         string v1 w2\n\
         count 3 1.25\n\
         block 36893488147419103274 2 -36893488147419103274 5 w1",
    );
}

const SLOT_OR_EMPTY: &str = r#"
use system.io
use system.collections.list

struct Triple
    a int
    b int
    c int

struct Block
    a i128
    b i128
    c i128
    d int

enum Slot<T>
    Filled(T, String)
    Empty
"#;

fn with_slot_or_empty(main: &str) -> String {
    format!("{SLOT_OR_EMPTY}\n{main}")
}

/// A generic function is compiled per instantiation, so the enum it builds is
/// laid out at the argument it was called with: a 128-bit value and a struct
/// held as its bytes each widen the first column, and the string after it is
/// stored, read and released where that wider column ends.
#[test]
fn test_generic_function_builds_enum_at_its_instantiation() {
    assert_heap_guard_output(
        &with_slot_or_empty(
            r#"
fn wrap<T>(x T) Slot<T>
    return Slot.Filled(x, "s" + "t")

fn main()
    let big i128 = 170141183460469231731687303715884105727
    match wrap(big)
        Slot.Filled(v, s): println(f"wide {v} {s}")
        Slot.Empty: println("empty")
    match wrap(Triple(a: 1, b: 2, c: 3))
        Slot.Filled(v, s): println(f"triple {v.a} {v.b} {v.c} {s}")
        Slot.Empty: println("empty")
"#,
        ),
        "wide 170141183460469231731687303715884105727 st\n\
         triple 1 2 3 st",
    );
}

#[test]
fn test_generic_method_builds_enum_at_its_instantiation() {
    assert_heap_guard_output(
        &with_slot_or_empty(
            r#"
class Boxer<T>
    var tag String

    fn wrap(x T) Slot<T>
        return Slot.Filled(x, self.tag + "!")

fn main()
    let big i128 = -170141183460469231731687303715884105727
    let wide = Boxer<i128>(tag: "w" + "1")
    match wide.wrap(big)
        Slot.Filled(v, s): println(f"wide {v} {s}")
        Slot.Empty: println("empty")
    let held = Boxer<Triple>(tag: "b" + "2")
    match held.wrap(Triple(a: 4, b: 5, c: 6))
        Slot.Filled(v, s): println(f"triple {v.a} {v.b} {v.c} {s}")
        Slot.Empty: println("empty")
"#,
        ),
        "wide -170141183460469231731687303715884105727 w1!\n\
         triple 4 5 6 b2!",
    );
}

/// A list of a generic enum releases each element through the drop thunk of
/// that enum's instantiation, which finds the string after the widened first
/// column where the construction put it.
#[test]
fn test_list_of_generic_enum_releases_after_a_widened_column() {
    assert_heap_guard_output(
        &with_slot_or_empty(
            r#"
fn main()
    var wides = List<Slot<i128>>()
    var i = 0
    while i < 3
        let big i128 = 170141183460469231731687303715884105727
        wides.push(Slot.Filled(big, f"s{i}"))
        i = i + 1
    wides.push(Slot.Empty)
    for s in wides
        match s
            Slot.Filled(v, t): println(f"wide {v} {t}")
            Slot.Empty: println("empty")
    var blocks = List<Slot<Block>>()
    var j = 0
    while j < 2
        blocks.push(Slot.Filled(Block(a: 36893488147419103274, b: 2, c: -3, d: j), f"b{j}"))
        j = j + 1
    for s in blocks
        match s
            Slot.Filled(v, t): println(f"block {v.a} {v.b} {v.c} {v.d} {t}")
            Slot.Empty: println("empty")
"#,
        ),
        "wide 170141183460469231731687303715884105727 s0\n\
         wide 170141183460469231731687303715884105727 s1\n\
         wide 170141183460469231731687303715884105727 s2\n\
         empty\n\
         block 36893488147419103274 2 -3 0 b0\n\
         block 36893488147419103274 2 -3 1 b1",
    );
}

#[test]
fn test_list_of_generic_enum_inside_a_generic_function() {
    assert_heap_guard_output(
        &with_slot_or_empty(
            r#"
fn tags<T>(x T, n int) String
    var xs = List<Slot<T>>()
    var i = 0
    while i < n
        xs.push(Slot.Filled(x, f"t{i}"))
        i = i + 1
    var out = ""
    for s in xs
        match s
            Slot.Filled(v, t): out = out + t
            Slot.Empty: out = out + "-"
    return out

fn main()
    let big i128 = 170141183460469231731687303715884105727
    println(tags(big, 3))
    println(tags("v" + "w", 2))
"#,
        ),
        "t0t1t2\nt0t1",
    );
}

/// The error parameter is left to inference; the value is still released at
/// the instantiation its bound argument names, past the 128-bit column.
#[test]
fn test_partially_inferred_generic_enum_releases_after_a_wide_column() {
    assert_heap_guard_output(
        r#"
use system.io

enum Res<T, E>
    Ok(T, String)
    Err(E)

fn main()
    let big i128 = -170141183460469231731687303715884105727
    let r = Res.Ok(big, "x" + "y")
    match r
        Res.Ok(v, t): println(f"ok {v} {t}")
        Res.Err(e): println("err")
"#,
        "ok -170141183460469231731687303715884105727 xy",
    );
}
