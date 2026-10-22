// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value held as its bytes is passed as the caller's own bytes, so an
//! optimized build that inlines its callees reads every field straight from
//! where the value already lives, with no copy through a stack slot.

use crate::utils::miri_cmd;

/// A vector normalized through two helpers that only read their arguments.
const NORMALIZE: &str = r#"use system.math

struct Vec3
    x float
    y float
    z float

fn dot(a Vec3, b Vec3) float
    a.x * b.x + a.y * b.y + a.z * b.z

fn scale(a Vec3, k float) Vec3
    Vec3(x: a.x * k, y: a.y * k, z: a.z * k)

fn normalize(a Vec3) Vec3
    scale(a, 1.0 / sqrt(dot(a, a)))

fn main()
    let v = normalize(Vec3(x: 3.0, y: 0.0, z: 4.0))
    println(f"{v.x} {v.y} {v.z}")
"#;

/// The CLIF listing an opt-level-3 build of `source` writes, and what the
/// built program prints.
fn optimized_listing_and_output(source: &str) -> (String, String) {
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
        .args(["--opt-level", "3", "--emit", "clif"])
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

#[test]
fn value_arguments_are_read_in_place_once_inlined() {
    let (listing, output) = optimized_listing_and_output(NORMALIZE);
    assert_eq!(output, "0.6000000000000001 0.0 0.8\n");
    let normalize = function_text(&listing, "miri.normalize");
    assert!(
        !normalize.contains("explicit_slot"),
        "normalize copies a value argument into a stack slot:\n{normalize}"
    );
    assert!(
        !normalize.contains("load.i64"),
        "normalize moves a value argument through integer words:\n{normalize}"
    );
}
