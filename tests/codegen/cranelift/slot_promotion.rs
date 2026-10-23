// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optimized build holds a value that lives as its bytes in registers
//! once every function reaching it is inlined: the per-sphere function of a
//! render kernel keeps none of its vectors on the stack, and computes what
//! the unoptimized build, which keeps them all there, does. Programs moving
//! structs through loops, joins and early exits print the same at every
//! optimization level.

use crate::utils::miri_cmd;

/// A render loop testing two spheres per pixel, `hit` inlining the vector
/// helpers it calls and branching on the discriminant.
const SPHERE_KERNEL: &str = r#"use system.io
use system.math

struct Vec3
    x float
    y float
    z float

struct Sphere
    center Vec3
    radius float

fn sub(a Vec3, b Vec3) Vec3: Vec3(x: a.x - b.x, y: a.y - b.y, z: a.z - b.z)
fn scale(a Vec3, k float) Vec3: Vec3(x: a.x * k, y: a.y * k, z: a.z * k)
fn dot(a Vec3, b Vec3) float: a.x * b.x + a.y * b.y + a.z * b.z
fn normalize(a Vec3) Vec3: scale(a, 1.0 / sqrt(dot(a, a)))

fn hit(sphere Sphere, direction Vec3) float
    let oc = scale(sphere.center, -1.0)
    let b = dot(oc, direction)
    let c = dot(oc, oc) - sphere.radius * sphere.radius
    let discriminant = b * b - c
    if discriminant < 0.0
        return -1.0
    -b - sqrt(discriminant)

fn kernel(n int) int
    let spheres = [Sphere(center: Vec3(x: 0.0, y: 0.0, z: 3.0), radius: 1.0), Sphere(center: Vec3(x: 1.0, y: 0.5, z: 4.0), radius: 0.5)]
    let light = normalize(Vec3(x: 1.0, y: 1.0, z: -1.0))
    var hash = 0
    var py = 0
    while py < n
        var px = 0
        while px < n
            let direction = normalize(Vec3(x: (px as float) / (n as float) - 0.5, y: 0.5 - (py as float) / (n as float), z: 1.0))
            var nearest = 1000000.0
            var shade = 0
            for sphere in spheres
                let t = hit(sphere, direction)
                if t > 0.0 and t < nearest
                    nearest = t
                    let normal = normalize(sub(scale(direction, t), sphere.center))
                    let lambert = dot(normal, light)
                    shade = if lambert > 0.0: (lambert * 255.0) as int else: 0
            hash = (hash * 31 + shade) % 1000000007
            px += 1
        py += 1
    hash

fn main()
    println(f"{kernel(40)}")
"#;

/// The CLIF listing a build of `source` at `opt_level` writes, and what the
/// built program prints.
pub(super) fn listing_and_output(source: &str, opt_level: &str) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("prog.mi");
    std::fs::write(&program, source).unwrap();
    let out = dir.path().join("prog");
    miri_cmd()
        .current_dir(dir.path())
        .arg("build")
        .arg(&program)
        .arg("--out")
        .arg(&out)
        .args(["--opt-level", opt_level, "--emit", "clif"])
        .assert()
        .success();
    let run = std::process::Command::new(&out).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let mut file = out.into_os_string();
    file.push(".clif");
    (
        std::fs::read_to_string(file).unwrap(),
        String::from_utf8(run.stdout).unwrap(),
    )
}

/// The text of the function `name` in `listing`.
pub(super) fn function_text<'a>(listing: &'a str, name: &str) -> &'a str {
    let header = format!("; {name}\n");
    let start = listing
        .find(&header)
        .unwrap_or_else(|| panic!("no function {name} in:\n{listing}"))
        + header.len();
    let rest = &listing[start..];
    let end = rest.find("\n\n; ").map_or(rest.len(), |end| end + 1);
    &rest[..end]
}

/// The sizes of the explicit stack slots `function` declares.
pub(super) fn slot_sizes(function: &str) -> Vec<u32> {
    function
        .lines()
        .filter_map(|line| line.split_once("= explicit_slot "))
        .map(|(_, rest)| {
            let size = rest.split([',', ' ']).next().unwrap();
            size.parse().unwrap()
        })
        .collect()
}

#[test]
fn the_inlined_per_sphere_function_keeps_none_of_its_vectors_on_the_stack() {
    let (optimized, _) = listing_and_output(SPHERE_KERNEL, "3");
    let hit = function_text(&optimized, "miri.hit");
    assert!(
        !hit.contains("stack_addr"),
        "a slot address is left in:\n{hit}"
    );
    assert!(
        slot_sizes(hit).iter().all(|&size| size == 0),
        "a slot keeps its bytes in:\n{hit}"
    );
    let (unoptimized, _) = listing_and_output(SPHERE_KERNEL, "0");
    let hit = function_text(&unoptimized, "miri.hit");
    assert!(
        slot_sizes(hit).iter().any(|&size| size != 0),
        "the unoptimized build no longer holds a vector in a slot:\n{hit}"
    );
}

#[test]
fn promoted_vectors_compute_what_vectors_on_the_stack_do() {
    let (_, unoptimized) = listing_and_output(SPHERE_KERNEL, "0");
    assert_eq!(unoptimized, "123496526\n");
    for opt_level in ["1", "3"] {
        let (_, optimized) = listing_and_output(SPHERE_KERNEL, opt_level);
        assert_eq!(optimized, unoptimized, "at opt level {opt_level}");
    }
}

/// A struct of an `int`, a `bool` and an `int`, every field mutated in a
/// loop the `bool` ends.
const FLAG_CONTROLLED_LOOP: &str = r#"use system.io

struct Counter
    count int
    active bool
    total int

fn main()
    var c = Counter(count: 0, active: true, total: 0)
    while c.active
        c.count += 1
        c.total += c.count * 2
        if c.count >= 10
            c.active = false
    println(f"{c.count} {c.active} {c.total}")
"#;

/// Early returns from nested `if`s reading a struct parameter.
const EARLY_RETURNS: &str = r#"use system.io

struct Box
    w int
    h int
    d int

fn classify(b Box) int
    if b.w > 10
        if b.h > 10
            return b.w + b.h
        if b.d > 10
            return b.d * 2
        return b.w
    if b.h == b.d
        return 0
    b.w + b.h + b.d

fn main()
    var sum = 0
    var i = 0
    while i < 20
        sum += classify(Box(w: i, h: 20 - i, d: i % 7 * 3))
        i += 1
    println(f"{sum}")
"#;

/// An optional field written on one arm of an `if` and read after the join.
const OPTIONAL_FIELD_AT_A_JOIN: &str = r#"use system.io

struct Slot
    id int
    value int?

fn main()
    var total = 0
    var i = 0
    while i < 10
        var s = Slot(id: i, value: None)
        if i % 3 == 0
            s.value = i * 10
        match s.value
            Some(v): total += v
            None: total += s.id
        i += 1
    println(f"{total}")
"#;

/// Two structs swapped through a temporary on every iteration.
const SWAP_THROUGH_A_TEMPORARY: &str = r#"use system.io

struct Pair
    a int
    b int

fn main()
    var p = Pair(a: 1, b: 2)
    var q = Pair(a: 3, b: 4)
    var i = 0
    while i < 5
        let t = p
        p = q
        q = Pair(a: t.a + i * 3, b: t.b * 2 + 1)
        i += 1
    println(f"{p.a} {p.b} {q.a} {q.b}")
"#;

/// A struct built on either arm of an inline `if`.
const STRUCT_FROM_EITHER_ARM: &str = r#"use system.io

struct Point
    x float
    y float

fn main()
    var sx = 0.0
    var sy = 0.0
    var i = 0
    while i < 8
        let r = if i % 2 == 0: Point(x: i as float, y: 1.5) else: Point(x: 0.5, y: i as float)
        sx += r.x
        sy += r.y * 2.0
        i += 1
    println(f"{sx} {sy}")
"#;

/// Fields of every width a struct holds, each updated in a loop.
const MIXED_WIDTH_FIELDS: &str = r#"use system.io

struct Mixed
    a i8
    b i16
    c i32
    d f32
    e float
    f bool

fn main()
    var m = Mixed(a: 1, b: 2, c: 3, d: 0.5, e: 0.25, f: false)
    var i = 0
    while i < 6
        m.a = m.a + 2
        m.b = m.b * 2
        m.c = m.c + m.b
        m.d = m.d * 2.0
        m.e = m.e + 0.5
        m.f = m.f == false
        i += 1
    println(f"{m.a} {m.b} {m.c} {m.d} {m.e} {m.f}")
"#;

/// A loop leaving by `break` and skipping by `continue`, a struct mutated
/// on each path.
const BREAK_AND_CONTINUE: &str = r#"use system.io

struct Acc
    evens int
    odds int
    steps int

fn main()
    var acc = Acc(evens: 0, odds: 0, steps: 0)
    var i = 0
    while true
        i += 1
        acc.steps += 1
        if i > 20
            break
        if i % 2 == 0
            acc.evens += i
            continue
        acc.odds += i
    println(f"{acc.evens} {acc.odds} {acc.steps}")
"#;

/// An outer struct local updated by an inner loop and read after it.
const NESTED_LOOP_ACCUMULATOR: &str = r#"use system.io

struct Stats
    sum int
    max int

fn main()
    var s = Stats(sum: 0, max: 0)
    var row = 0
    var checksum = 0
    while row < 5
        var col = 0
        while col < 4
            let v = row * 7 + col * 3 % 5
            s.sum += v
            if v > s.max
                s.max = v
            col += 1
        checksum = checksum * 31 + s.sum + s.max
        row += 1
    println(f"{s.sum} {s.max} {checksum}")
"#;

/// Structs handed to small helpers an optimized build inlines.
const INLINED_HELPERS: &str = r#"use system.io

struct Vec2
    x int
    y int

fn add(a Vec2, b Vec2) Vec2: Vec2(x: a.x + b.x, y: a.y + b.y)
fn cross(a Vec2, b Vec2) int: a.x * b.y - a.y * b.x

fn main()
    var acc = Vec2(x: 0, y: 0)
    var area = 0
    var i = 0
    while i < 10
        let step = Vec2(x: i, y: 10 - i)
        area += cross(acc, step)
        acc = add(acc, step)
        i += 1
    println(f"{acc.x} {acc.y} {area}")
"#;

/// Structs pushed into a list and read back out of it.
const STRUCTS_IN_A_LIST: &str = r#"use system.io
use system.collections.list

struct Item
    weight int
    value int

fn main()
    var items = List<Item>()
    var i = 0
    while i < 6
        items.push(Item(weight: i + 1, value: (i * 13) % 7))
        i += 1
    var best = Item(weight: 0, value: -1)
    var total = 0
    for item in items
        total += item.weight * item.value
        if item.value > best.value
            best = item
    println(f"{items.length()} {total} {best.weight} {best.value}")
"#;

/// Each program, with what it must print.
const DIFFERENTIAL_PROGRAMS: [(&str, &str, &str); 10] = [
    (
        "flag-controlled loop",
        FLAG_CONTROLLED_LOOP,
        "10 false 110\n",
    ),
    ("early returns", EARLY_RETURNS, "472\n"),
    (
        "optional field at a join",
        OPTIONAL_FIELD_AT_A_JOIN,
        "207\n",
    ),
    (
        "swap through a temporary",
        SWAP_THROUGH_A_TEMPORARY,
        "15 19 19 23\n",
    ),
    (
        "struct from either arm",
        STRUCT_FROM_EITHER_ARM,
        "14.0 44.0\n",
    ),
    (
        "mixed-width fields",
        MIXED_WIDTH_FIELDS,
        "13 128 255 32.0 3.25 false\n",
    ),
    ("break and continue", BREAK_AND_CONTINUE, "110 100 21\n"),
    (
        "nested loop accumulator",
        NESTED_LOOP_ACCUMULATOR,
        "320 32 12849170\n",
    ),
    ("inlined helpers", INLINED_HELPERS, "45 55 -1650\n"),
    ("structs in a list", STRUCTS_IN_A_LIST, "6 70 2 6\n"),
];

#[test]
fn structs_through_loops_joins_and_early_exits_print_the_same_at_every_opt_level() {
    for (name, source, expected) in DIFFERENTIAL_PROGRAMS {
        for opt_level in ["0", "1", "3"] {
            let (_, output) = listing_and_output(source, opt_level);
            assert_eq!(output, expected, "{name} at opt level {opt_level}");
        }
    }
}

#[test]
fn a_struct_local_mutated_in_a_loop_leaves_no_slot_in_main() {
    let (optimized, _) = listing_and_output(FLAG_CONTROLLED_LOOP, "3");
    let main = function_text(&optimized, "main");
    assert!(
        !main.contains("stack_addr"),
        "a slot address is left in:\n{main}"
    );
    assert!(
        slot_sizes(main).iter().all(|&size| size == 0),
        "a slot keeps its bytes in:\n{main}"
    );
}
