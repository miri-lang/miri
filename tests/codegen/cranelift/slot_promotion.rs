// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optimized build holds a value that lives as its bytes in registers
//! once every function reaching it is inlined: the per-sphere function of a
//! render kernel keeps none of its vectors on the stack, and computes what
//! the unoptimized build, which keeps them all there, does.

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
fn listing_and_output(source: &str, opt_level: &str) -> (String, String) {
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

/// The sizes of the explicit stack slots `function` declares.
fn slot_sizes(function: &str) -> Vec<u32> {
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
fn an_inlined_hot_function_keeps_none_of_its_vectors_on_the_stack() {
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
