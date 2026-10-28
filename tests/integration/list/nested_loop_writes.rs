// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lists written on every iteration of an inner loop. The compiler makes such
//! a list its own once, before the outermost loop whose every iteration runs
//! the inner one, rather than once per run of the inner loop. Each program
//! pins what it prints, under the heap guard too: a list the caller still
//! holds is copied before the first write and keeps its values, whether the
//! outer or the inner loop runs no iteration at all.

use super::utils::*;

const PREAMBLE: &str = "use system.io\nuse system.collections.list\n";

fn expect_output(body: &str, expected: &str) {
    let source = format!("{PREAMBLE}\n{body}");
    assert_runs_with_output(&source, expected);
    assert_heap_guard_output(&source, expected);
}

const SPREAD: &str = r#"
fn spread(start [int], rounds int, width int) [int]
    var counts = start
    var i = 0
    while i < rounds
        var j = 0
        while j < width
            counts[j % 4] = counts[j % 4] + i + 1
            j = j + 1
        i = i + 1
    counts
"#;

#[test]
fn test_a_list_the_caller_holds_is_copied_before_a_nest_writes_it() {
    let main = r#"
fn main()
    let held = List([1, 2, 3, 4])
    let written = spread(held, 3, 6)
    println(f"{held[0]} {held[1]} {held[2]} {held[3]}")
    println(f"{written[0]} {written[1]} {written[2]} {written[3]}")
"#;
    expect_output(&format!("{SPREAD}{main}"), "1 2 3 4\n13 14 9 10\n");
}

#[test]
fn test_a_nest_whose_outer_loop_runs_no_iteration_leaves_both_lists_unchanged() {
    let main = r#"
fn main()
    let held = List([1, 2, 3, 4])
    let written = spread(held, 0, 6)
    println(f"{held[0]} {held[3]} {written[0]} {written[3]} {written.length()}")
"#;
    expect_output(&format!("{SPREAD}{main}"), "1 4 1 4 4\n");
}

#[test]
fn test_a_nest_whose_inner_loop_runs_no_iteration_leaves_both_lists_unchanged() {
    let main = r#"
fn main()
    let held = List([1, 2, 3, 4])
    let written = spread(held, 5, 0)
    println(f"{held[0]} {held[3]} {written[0]} {written[3]} {written.length()}")
"#;
    expect_output(&format!("{SPREAD}{main}"), "1 4 1 4 4\n");
}

#[test]
fn test_a_snapshot_taken_in_the_outer_loop_keeps_the_values_of_its_round() {
    let program = r#"
fn rounds(n int) int
    var counts = List([0, 0, 0, 0])
    var total = 0
    var i = 0
    while i < n
        let snapshot = counts
        var j = 0
        while j < 4
            counts[j] = snapshot[j] + i + 1
            j = j + 1
        total = total * 10 + snapshot[0]
        i = i + 1
    total * 10 + counts[0]

fn main()
    println(f"{rounds(4)}")
"#;
    expect_output(program, "1370\n");
}

#[test]
fn test_a_list_rebound_in_the_outer_loop_is_copied_in_every_round() {
    let program = r#"
fn rounds(start [int], n int) int
    var counts = List([0, 0, 0, 0])
    var total = 0
    var i = 0
    while i < n
        counts = start
        var j = 0
        while j < 4
            counts[j] = counts[j] + i + 1
            j = j + 1
        total = total * 10 + counts[0]
        i = i + 1
    total * 10 + start[0]

fn main()
    let start = List([1, 1, 1, 1])
    println(f"{rounds(start, 3)} {start[0]} {start[3]}")
"#;
    expect_output(program, "2341 1 1\n");
}

/// The n-body step of the benchmark, whose outer loop runs the pair loop
/// writing the bodies on every iteration, followed by the loop moving them.
const NBODY: &str = r#"
struct Body
    x float
    y float
    z float
    vx float
    vy float
    vz float
    mass float

fn solar_mass() float: 4.0 * 3.141592653589793 * 3.141592653589793

fn days_per_year() float: 365.24

fn planet(x float, y float, z float, vx float, vy float, vz float, mass float) Body
    let d = days_per_year()
    Body(x: x, y: y, z: z, vx: vx * d, vy: vy * d, vz: vz * d, mass: mass * solar_mass())

fn solar_system() [Body]
    List([
        Body(x: 0.0, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, mass: solar_mass()),
        planet(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04),
        planet(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04),
        planet(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05),
        planet(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05)
    ])

fn offset_momentum(start [Body]) [Body]
    var bodies = start
    var px = 0.0
    var py = 0.0
    var pz = 0.0
    for b in bodies
        px = px + b.vx * b.mass
        py = py + b.vy * b.mass
        pz = pz + b.vz * b.mass
    bodies[0].vx = -px / solar_mass()
    bodies[0].vy = -py / solar_mass()
    bodies[0].vz = -pz / solar_mass()
    bodies

fn energy(bodies [Body]) float
    var e = 0.0
    let n = bodies.length()
    var i = 0
    while i < n
        let b = bodies[i]
        e = e + 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz)
        var j = i + 1
        while j < n
            let o = bodies[j]
            let dx = b.x - o.x
            let dy = b.y - o.y
            let dz = b.z - o.z
            e = e - b.mass * o.mass / sqrt(dx * dx + dy * dy + dz * dz)
            j += 1
        i += 1
    e

fn advance(start [Body], dt float) [Body]
    var bodies = start
    let n = bodies.length()
    var i = 0
    while i < n
        var j = i + 1
        while j < n
            let dx = bodies[i].x - bodies[j].x
            let dy = bodies[i].y - bodies[j].y
            let dz = bodies[i].z - bodies[j].z
            let d2 = dx * dx + dy * dy + dz * dz
            let mag = dt / (d2 * sqrt(d2))
            let mi = bodies[i].mass * mag
            let mj = bodies[j].mass * mag
            bodies[i].vx = bodies[i].vx - dx * mj
            bodies[i].vy = bodies[i].vy - dy * mj
            bodies[i].vz = bodies[i].vz - dz * mj
            bodies[j].vx = bodies[j].vx + dx * mi
            bodies[j].vy = bodies[j].vy + dy * mi
            bodies[j].vz = bodies[j].vz + dz * mi
            j += 1
        i += 1
    i = 0
    while i < n
        bodies[i].x = bodies[i].x + dt * bodies[i].vx
        bodies[i].y = bodies[i].y + dt * bodies[i].vy
        bodies[i].z = bodies[i].z + dt * bodies[i].vz
        i += 1
    bodies

fn main()
    let first = offset_momentum(solar_system())
    var bodies = first
    let before = energy(bodies)
    var s = 0
    while s < 1000
        bodies = advance(bodies, 0.01)
        s += 1
    let after = energy(bodies)
    println(f"{(before * 1000000000.0) as int} {(after * 1000000000.0) as int}")
    println(f"{(energy(first) * 1000000000.0) as int}")
"#;

/// The energies the benchmark's other ports print after 1000 steps, with
/// the energy of the bodies the run started from, which it still holds.
#[test]
fn test_the_nbody_step_reaches_the_energy_every_port_reaches() {
    let source = format!("{PREAMBLE}use system.math\n{NBODY}");
    let expected = "-169075163 -169087605\n-169075163\n";
    assert_runs_with_output(&source, expected);
    assert_heap_guard_output(&source, expected);
}
