// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optimized build inlines a small, loop-free user function into each of
//! its callers; an unoptimized build calls it. Recursive functions and
//! functions holding a loop keep their calls, and inlined code that refers
//! to a string literal or other static data still reaches the right symbol.

use crate::utils::miri_cmd;
use std::path::{Path, PathBuf};

/// A tiny helper called from a loop.
const TINY_HELPER: &str = r#"use system.io

fn twice_plus_one(x int) int
    x * 2 + 1

fn main()
    var total = 0
    var i = 0
    while i < 10
        total += twice_plus_one(i)
        i += 1
    println(f"{total}")
"#;

/// Helpers whose bodies reach static data: a string literal and an
/// interpolated string, the latter called once so its size is held to the
/// single call site's limit.
const STRING_HELPERS: &str = r#"use system.io

fn greeting() String
    "hello"

fn label(n int) String
    f"n={n}"

fn main()
    println(greeting())
    println(label(7))
    println(f"{greeting()} !")
"#;

/// A self-recursive function and a mutually recursive pair.
const RECURSIVE_HELPERS: &str = r#"use system.io

fn factorial(n int) int
    if n <= 1
        return 1
    n * factorial(n - 1)

fn is_even(n int) bool
    if n == 0
        return true
    is_odd(n - 1)

fn is_odd(n int) bool
    if n == 0
        return false
    is_even(n - 1)

fn main()
    println(f"{factorial(10)} {is_even(10)} {is_odd(7)}")
"#;

/// A small helper whose body is a loop.
const LOOPING_HELPER: &str = r#"use system.io

fn sum_to(n int) int
    var total = 0
    var i = 0
    while i < n
        total += i
        i += 1
    total

fn main()
    println(f"{sum_to(10)}")
"#;

/// A small render loop calling five small vector helpers, two of which
/// call the others: the shape of a per-pixel kernel.
const VECTOR_KERNEL: &str = r#"use system.io
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

/// Write `source` into `dir` and build it with `flags`, returning the
/// executable's path.
fn build(dir: &Path, source: &str, flags: &[&str]) -> PathBuf {
    let program = dir.join("prog.mi");
    std::fs::write(&program, source).unwrap();
    let out = dir.join("prog");
    miri_cmd()
        .current_dir(dir)
        .arg("build")
        .arg(&program)
        .arg("--out")
        .arg(&out)
        .args(flags)
        .assert()
        .success();
    out
}

/// The CLIF listing a build of `source` at `opt_level` writes.
fn listing(source: &str, opt_level: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = build(
        dir.path(),
        source,
        &["--opt-level", opt_level, "--emit", "clif"],
    );
    let mut file = out.into_os_string();
    file.push(".clif");
    std::fs::read_to_string(file).unwrap()
}

/// The text of the function `name` in `listing`: its CLIF and the comment
/// lines naming what it refers to.
fn function_text<'a>(listing: &'a str, name: &str) -> &'a str {
    let header = format!("; {name}\n");
    let start = listing
        .find(&header)
        .unwrap_or_else(|| panic!("no function {name} in:\n{listing}"))
        + header.len();
    let rest = &listing[start..];
    let end = rest.find("\n\n; ").map_or(rest.len(), |end| end + 1);
    &rest[..end]
}

/// Whether the function `caller` in `listing` calls the function `callee`.
fn calls(listing: &str, caller: &str, callee: &str) -> bool {
    let text = function_text(listing, caller);
    let handles: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("; "))
        .filter_map(|reference| reference.split_once(" = "))
        .filter(|(_, target)| *target == callee)
        .map(|(handle, _)| handle)
        .collect();
    handles.iter().any(|handle| {
        text.lines()
            .any(|line| line.contains(&format!("call {handle}(")))
    })
}

/// What running `executable` printed, after checking it succeeded.
fn run(executable: &Path) -> String {
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn build_and_run(source: &str, flags: &[&str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    run(&build(dir.path(), source, flags))
}

#[test]
fn an_optimized_build_inlines_a_tiny_helper() {
    let optimized = listing(TINY_HELPER, "3");
    assert!(
        !calls(&optimized, "main", "miri.twice_plus_one"),
        "the helper was not inlined at opt level 3:\n{optimized}"
    );
    let unoptimized = listing(TINY_HELPER, "0");
    assert!(
        calls(&unoptimized, "main", "miri.twice_plus_one"),
        "the helper was inlined at opt level 0:\n{unoptimized}"
    );
    assert_eq!(build_and_run(TINY_HELPER, &["--opt-level", "3"]), "100\n");
}

#[test]
fn inlined_helpers_reach_their_string_literals() {
    let unoptimized = listing(STRING_HELPERS, "0");
    for helper in ["miri.greeting", "miri.label"] {
        assert!(
            calls(&unoptimized, "main", helper),
            "{helper} is not called at opt level 0:\n{unoptimized}"
        );
    }
    let optimized = listing(STRING_HELPERS, "3");
    for helper in ["miri.greeting", "miri.label"] {
        assert!(
            !calls(&optimized, "main", helper),
            "{helper} was not inlined:\n{optimized}"
        );
    }
    for flags in [&["--opt-level", "3"][..], &["--release"]] {
        assert_eq!(
            build_and_run(STRING_HELPERS, flags),
            "hello\nn=7\nhello !\n",
            "with {flags:?}"
        );
    }
}

#[test]
fn recursive_helpers_compile_and_run_optimized() {
    let optimized = listing(RECURSIVE_HELPERS, "3");
    for (caller, callee) in [
        ("miri.factorial", "miri.factorial"),
        ("miri.is_even", "miri.is_odd"),
        ("miri.is_odd", "miri.is_even"),
    ] {
        assert!(
            calls(&optimized, caller, callee),
            "{caller} no longer calls {callee}:\n{optimized}"
        );
    }
    assert_eq!(
        build_and_run(RECURSIVE_HELPERS, &["--opt-level", "3"]),
        "3628800 true true\n"
    );
}

#[test]
fn a_helper_holding_a_loop_keeps_its_call() {
    let optimized = listing(LOOPING_HELPER, "3");
    assert!(
        calls(&optimized, "main", "miri.sum_to"),
        "the looping helper was inlined:\n{optimized}"
    );
    assert_eq!(build_and_run(LOOPING_HELPER, &["--opt-level", "3"]), "45\n");
}

/// A helper whose addition overflows when called with the largest `int`.
const OVERFLOWING_HELPER: &str = r#"use system.io

fn bump(x int) int
    x + 1

fn main()
    var x = 9223372036854775800
    var i = 0
    while i < 10
        x = bump(x)
        i += 1
    println(f"{x}")
"#;

#[test]
fn an_inlined_helper_still_traps_on_overflow() {
    let unoptimized = listing(OVERFLOWING_HELPER, "0");
    assert!(
        calls(&unoptimized, "main", "miri.bump"),
        "the helper is not called at opt level 0:\n{unoptimized}"
    );
    let optimized = listing(OVERFLOWING_HELPER, "3");
    assert!(
        !calls(&optimized, "main", "miri.bump"),
        "the helper was not inlined:\n{optimized}"
    );
    let dir = tempfile::tempdir().unwrap();
    let executable = build(dir.path(), OVERFLOWING_HELPER, &["--opt-level", "3"]);
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(!output.status.success(), "the overflow did not trap");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("overflow"), "{stderr}");
}

/// Every optimizing level inlines, the lowest included.
#[test]
fn opt_level_1_inlines_a_tiny_helper_too() {
    let optimized = listing(TINY_HELPER, "1");
    assert!(
        !calls(&optimized, "main", "miri.twice_plus_one"),
        "the helper was not inlined at opt level 1:\n{optimized}"
    );
    assert_eq!(build_and_run(TINY_HELPER, &["--opt-level", "1"]), "100\n");
}

/// The helpers a per-pixel kernel calls on every pixel and sphere, one of
/// them calling the others, are inlined into it once value arguments are read
/// in place, and the inlined kernel computes what the unoptimized one does.
#[test]
fn a_render_kernel_inlines_its_per_pixel_helpers() {
    let optimized = listing(VECTOR_KERNEL, "3");
    for helper in ["normalize", "hit", "dot", "sub"] {
        assert!(
            !calls(&optimized, "miri.kernel", &format!("miri.{helper}")),
            "the kernel still calls {helper}:\n{}",
            function_text(&optimized, "miri.kernel")
        );
    }
    assert_eq!(
        build_and_run(VECTOR_KERNEL, &["--opt-level", "3"]),
        "123496526\n"
    );
    assert_eq!(
        build_and_run(VECTOR_KERNEL, &["--opt-level", "0"]),
        "123496526\n"
    );
}
