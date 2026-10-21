// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The shape of the code an element access compiles to: which header words of
//! the collection it reads, and how often, as `--emit clif` shows them.

use crate::utils::miri_cmd;
use std::io::Write;
use std::path::PathBuf;

/// A list of value structs written through a path in a pair of nested loops,
/// then in a second loop.
const NBODY_SHAPED: &str = r#"
use system.io
use system.collections.list

struct Body
    x float
    vx float
    mass float

fn step(start [Body], dt float) [Body]
    var bodies = start
    let n = bodies.length()
    var i = 0
    while i < n
        var j = i + 1
        while j < n
            let dx = bodies[i].x - bodies[j].x
            let mi = bodies[i].mass * dx
            let mj = bodies[j].mass * dx
            bodies[i].vx = bodies[i].vx - dt * mj
            bodies[j].vx = bodies[j].vx + dt * mi
            j += 1
        i += 1
    i = 0
    while i < n
        bodies[i].x = bodies[i].x + dt * bodies[i].vx
        i += 1
    bodies

fn main()
    var bodies = List([Body(x: 0.0, vx: 1.0, mass: 2.0), Body(x: 1.0, vx: 0.5, mass: 1.0)])
    bodies = step(bodies, 0.5)
    println(f"{bodies[0].x} {bodies[1].x}")
"#;

/// A list of flags read once in a loop and read and written at a stride in a
/// loop nested in it.
const SIEVE_SHAPED: &str = r#"
use system.io
use system.collections.list

fn count_primes(n int) int
    var composite = List<bool>()
    composite.resize(n + 1, false)
    var count = 0
    var gaps = 0
    var p = 2
    while p <= n
        if not composite[p]
            count += 1
            var m = p * p
            while m <= n
                if not composite[m - 1]
                    gaps += 1
                composite[m] = true
                m += p
        p += 1
    count + gaps

fn main()
    println(f"{count_primes(100)}")
"#;

/// The CLIF of the function whose link name ends in `.name`, built from
/// `source` as a release build.
fn clif_of(source: &str, name: &str) -> String {
    let mut file = tempfile::Builder::new().suffix(".mi").tempfile().unwrap();
    write!(file, "{source}").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out: PathBuf = out_dir.path().join("shape");
    miri_cmd()
        .arg("build")
        .arg(file.path())
        .arg("--release")
        .arg("--out")
        .arg(&out)
        .args(["--emit", "clif"])
        .assert()
        .success();
    let clif = std::fs::read_to_string(out.with_extension("clif")).unwrap();
    let marker = format!("; miri.{name}\n");
    let start = clif
        .find(&marker)
        .unwrap_or_else(|| panic!("no function {name} in:\n{clif}"));
    let rest = &clif[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |end| end + 3);
    rest[..end].to_string()
}

/// The loads in `clif` of the word at `offset` bytes into a header.
fn header_loads_at(clif: &str, offset: i32) -> usize {
    clif.lines()
        .filter_map(|line| line.split_once(" = load.i64 ").map(|(_, load)| load))
        .filter_map(|load| load.split_whitespace().find(|word| word.starts_with('v')))
        .filter(|address| {
            let at = address
                .split_once('+')
                .map_or(Some(0), |(_, at)| at.parse::<i32>().ok());
            at == Some(offset)
        })
        .count()
}

#[test]
fn test_a_slot_size_mismatch_traps_from_a_cold_block_not_a_conditional_trap() {
    let clif = clif_of(NBODY_SHAPED, "step");
    assert!(
        !clif.contains("trapnz"),
        "a slot-size check is a conditional trap, a fence for every load after it:\n{clif}"
    );
    assert!(
        clif.contains("trap user3"),
        "the slot-size check is gone:\n{clif}"
    );
}

#[test]
fn test_a_loop_reads_the_length_of_a_list_it_leaves_unchanged_once() {
    let clif = clif_of(SIEVE_SHAPED, "count_primes");
    // One read of the length for the outer loop's access, which shares the
    // loop with a copy-on-write that may replace the list, and one before
    // the inner loop for both of its accesses.
    assert_eq!(header_loads_at(&clif, 8), 2, "{clif}");
}

#[test]
fn test_a_pair_loop_writing_through_a_list_of_structs_reads_its_header_once_per_loop() {
    let clif = clif_of(NBODY_SHAPED, "step");
    // The slot size is checked once before the inner pair loop and once
    // before the second loop, rather than at each of their accesses.
    assert_eq!(header_loads_at(&clif, 24), 2, "{clif}");
}

#[test]
fn test_a_pair_loop_guarded_by_the_length_compares_no_index_to_it() {
    let clif = clif_of(NBODY_SHAPED, "step");
    assert_eq!(
        clif.matches("icmp.i64 uge").count() + clif.matches("icmp uge").count(),
        0,
        "an index the guard proves is still compared:\n{clif}"
    );
    assert!(
        clif.contains("sadd_overflow"),
        "the induction step no longer traps on overflow:\n{clif}"
    );
}

#[test]
fn test_a_loop_guarded_by_less_or_equal_keeps_its_bounds_check() {
    let clif = clif_of(SIEVE_SHAPED, "count_primes");
    assert!(
        clif.contains("icmp.i64 uge") || clif.contains("icmp uge"),
        "`m <= n` proves nothing, yet the check is gone:\n{clif}"
    );
}
