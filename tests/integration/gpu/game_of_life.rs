// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Game of Life rule correctness tests.
//!
//! These tests verify the implementation of Conway's Game of Life B3/S23 rule
//! on GPU kernels. The rule is:
//! - A live cell with 2 or 3 live neighbors survives.
//! - A dead cell with exactly 3 live neighbors is born.
//! - All other cells die.

use super::device::assert_gpu_runs_with_output;

/// Test the B3/S23 rule with an isolated blinker oscillator.
///
/// A blinker is a period-2 oscillator that alternates between horizontal
/// and vertical orientations. This test verifies that:
/// - Gen 1: horizontal (row 0, cols 0-2) → vertical (col 1, rows -1 to 1)
/// - Gen 2: vertical → horizontal
/// - Gen 3: horizontal → vertical
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_blinker_oscillation() {
    let source = r#"
use system.collections.array

gpu var grid = Array<int, 25>()

// Initialize a horizontal blinker in a 5x5 grid at row 2, cols 1-3
gpu forall i in 0..25
    var v = 0
    if i == 2 * 5 + 1
        v = 1
    if i == 2 * 5 + 2
        v = 1
    if i == 2 * 5 + 3
        v = 1
    grid[i] = v

// Generation 1: horizontal → vertical
gpu var next_grid = Array<int, 25>()
gpu forall i in 0..25
    let y = i / 5
    let x = i - y * 5
    let ym = (y - 1 + 5) % 5
    let yp = (y + 1) % 5
    let xm = (x - 1 + 5) % 5
    let xp = (x + 1) % 5
    let n = grid[ym*5+xm] + grid[ym*5+x] + grid[ym*5+xp] + grid[y*5+xm] + grid[y*5+xp] + grid[yp*5+xm] + grid[yp*5+x] + grid[yp*5+xp]
    var state = 0
    if grid[i] == 1
        if n == 2
            state = 1
        if n == 3
            state = 1
    if grid[i] == 0
        if n == 3
            state = 1
    next_grid[i] = state

// Generation 2: vertical → horizontal
gpu var temp = Array<int, 25>()
gpu forall i in 0..25
    let y = i / 5
    let x = i - y * 5
    let ym = (y - 1 + 5) % 5
    let yp = (y + 1) % 5
    let xm = (x - 1 + 5) % 5
    let xp = (x + 1) % 5
    let n = next_grid[ym*5+xm] + next_grid[ym*5+x] + next_grid[ym*5+xp] + next_grid[y*5+xm] + next_grid[y*5+xp] + next_grid[yp*5+xm] + next_grid[yp*5+x] + next_grid[yp*5+xp]
    var state = 0
    if next_grid[i] == 1
        if n == 2
            state = 1
        if n == 3
            state = 1
    if next_grid[i] == 0
        if n == 3
            state = 1
    temp[i] = state

// Generation 3: horizontal → vertical
gpu forall i in 0..25
    let y = i / 5
    let x = i - y * 5
    let ym = (y - 1 + 5) % 5
    let yp = (y + 1) % 5
    let xm = (x - 1 + 5) % 5
    let xp = (x + 1) % 5
    let n = temp[ym*5+xm] + temp[ym*5+x] + temp[ym*5+xp] + temp[y*5+xm] + temp[y*5+xp] + temp[yp*5+xm] + temp[yp*5+x] + temp[yp*5+xp]
    var state = 0
    if temp[i] == 1
        if n == 2
            state = 1
        if n == 3
            state = 1
    if temp[i] == 0
        if n == 3
            state = 1
    grid[i] = state

// Readback to host
let host = grid

// Verify: after 3 gens (odd), should be vertical at col 2, rows 1-3
let h_top = host[1 * 5 + 2]
let h_mid = host[2 * 5 + 2]
let h_bot = host[3 * 5 + 2]
let v_left = host[2 * 5 + 1]
let v_right = host[2 * 5 + 3]

println(f"{h_top} {h_mid} {h_bot} {v_left} {v_right}")
"#;
    assert_gpu_runs_with_output(source, "1 1 1 0 0");
}

/// Test the still-life property of a 2x2 block.
///
/// A block is a 2x2 square of live cells. It is a still-life configuration
/// that should remain unchanged across generations.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_block_still_life() {
    let source = r#"
use system.collections.array

gpu var grid = Array<int, 16>()

// Initialize a 2x2 block in a 4x4 grid at (1,1)-(2,2)
gpu forall i in 0..16
    var v = 0
    if i == 1 * 4 + 1
        v = 1
    if i == 1 * 4 + 2
        v = 1
    if i == 2 * 4 + 1
        v = 1
    if i == 2 * 4 + 2
        v = 1
    grid[i] = v

// One generation of evolution
gpu var next = Array<int, 16>()
gpu forall i in 0..16
    let y = i / 4
    let x = i - y * 4
    let ym = (y - 1 + 4) % 4
    let yp = (y + 1) % 4
    let xm = (x - 1 + 4) % 4
    let xp = (x + 1) % 4
    let n = grid[ym*4+xm] + grid[ym*4+x] + grid[ym*4+xp] + grid[y*4+xm] + grid[y*4+xp] + grid[yp*4+xm] + grid[yp*4+x] + grid[yp*4+xp]
    var state = 0
    if grid[i] == 1
        if n == 2
            state = 1
        if n == 3
            state = 1
    if grid[i] == 0
        if n == 3
            state = 1
    next[i] = state

// Readback to host
let host = next

// Verify: block should be unchanged
let v00 = host[1 * 4 + 1]
let v01 = host[1 * 4 + 2]
let v10 = host[2 * 4 + 1]
let v11 = host[2 * 4 + 2]

println(f"{v00} {v01} {v10} {v11}")
"#;
    assert_gpu_runs_with_output(source, "1 1 1 1");
}

/// Test the glider spaceship pattern.
///
/// A glider is a 5-cell pattern that translates across the grid diagonally.
/// Every 4 generations, it moves (+1 row, +1 col), returning to the same
/// orientation. This test verifies the glider's canonical shape and movement.
///
/// Canonical glider at (1,1)-(2,2) area (in a 5x5 grid):
/// Gen 0: (0,1), (1,2), (2,0), (2,1), (2,2)
/// Gen 4: (1,2), (2,3), (3,1), (3,2), (3,3) — translated (+1, +1)
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_glider_spaceship() {
    let source = r#"
use system.collections.array

gpu var grid = Array<int, 25>()

// Initialize canonical glider in a 5x5 grid
gpu forall i in 0..25
    var v = 0
    if i == 0 * 5 + 1
        v = 1
    if i == 1 * 5 + 2
        v = 1
    if i == 2 * 5 + 0
        v = 1
    if i == 2 * 5 + 1
        v = 1
    if i == 2 * 5 + 2
        v = 1
    grid[i] = v

// Two iterations of ping-pong evolution (4 generations total)
gpu var next = Array<int, 25>()
var gen = 0
while gen < 2
    // Read grid, write next
    gpu forall i in 0..25
        let y = i / 5
        let x = i - y * 5
        let ym = (y - 1 + 5) % 5
        let yp = (y + 1) % 5
        let xm = (x - 1 + 5) % 5
        let xp = (x + 1) % 5
        let n = grid[ym*5+xm] + grid[ym*5+x] + grid[ym*5+xp] + grid[y*5+xm] + grid[y*5+xp] + grid[yp*5+xm] + grid[yp*5+x] + grid[yp*5+xp]
        var state = 0
        if grid[i] == 1
            if n == 2
                state = 1
            if n == 3
                state = 1
        if grid[i] == 0
            if n == 3
                state = 1
        next[i] = state

    // Read next, write grid
    gpu forall i in 0..25
        let y = i / 5
        let x = i - y * 5
        let ym = (y - 1 + 5) % 5
        let yp = (y + 1) % 5
        let xm = (x - 1 + 5) % 5
        let xp = (x + 1) % 5
        let n = next[ym*5+xm] + next[ym*5+x] + next[ym*5+xp] + next[y*5+xm] + next[y*5+xp] + next[yp*5+xm] + next[yp*5+x] + next[yp*5+xp]
        var state = 0
        if next[i] == 1
            if n == 2
                state = 1
            if n == 3
                state = 1
        if next[i] == 0
            if n == 3
                state = 1
        grid[i] = state

    gen = gen + 1

// Readback after 4 generations
let host = grid

// Verify glider has translated (+1 row, +1 col) to (1,2)-(3,3) area
let g1 = host[1 * 5 + 2]
let g2 = host[2 * 5 + 3]
let g3 = host[3 * 5 + 1]
let g4 = host[3 * 5 + 2]
let g5 = host[3 * 5 + 3]

// Verify original glider cells are now dead
let old1 = host[0 * 5 + 1]
let old2 = host[2 * 5 + 0]

println(f"{g1}{g2}{g3}{g4}{g5}{old1}{old2}")
"#;
    assert_gpu_runs_with_output(source, "1111100");
}

/// Test glider live-cell census across the entire grid.
///
/// A glider is always exactly 5 cells. This test verifies that after 4 generations,
/// the total count of live cells in the entire 5x5 grid equals exactly 5. This is
/// a safety net against too-permissive birth rules (e.g., B23 instead of B3)
/// that would create spurious births in unchecked regions of the grid.
///
/// The census is computed with a host-side loop after readback, ensuring the
/// rule application is correct across the entire grid, not just the glider region.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_glider_live_cell_census() {
    let source = r#"
use system.collections.array

gpu var grid = Array<int, 25>()

// Initialize canonical glider in a 5x5 grid
gpu forall i in 0..25
    var v = 0
    if i == 0 * 5 + 1
        v = 1
    if i == 1 * 5 + 2
        v = 1
    if i == 2 * 5 + 0
        v = 1
    if i == 2 * 5 + 1
        v = 1
    if i == 2 * 5 + 2
        v = 1
    grid[i] = v

// Two iterations of ping-pong evolution (4 generations total)
gpu var next = Array<int, 25>()
var gen = 0
while gen < 2
    // Read grid, write next
    gpu forall i in 0..25
        let y = i / 5
        let x = i - y * 5
        let ym = (y - 1 + 5) % 5
        let yp = (y + 1) % 5
        let xm = (x - 1 + 5) % 5
        let xp = (x + 1) % 5
        let n = grid[ym*5+xm] + grid[ym*5+x] + grid[ym*5+xp] + grid[y*5+xm] + grid[y*5+xp] + grid[yp*5+xm] + grid[yp*5+x] + grid[yp*5+xp]
        var state = 0
        if grid[i] == 1
            if n == 2
                state = 1
            if n == 3
                state = 1
        if grid[i] == 0
            if n == 3
                state = 1
        next[i] = state

    // Read next, write grid
    gpu forall i in 0..25
        let y = i / 5
        let x = i - y * 5
        let ym = (y - 1 + 5) % 5
        let yp = (y + 1) % 5
        let xm = (x - 1 + 5) % 5
        let xp = (x + 1) % 5
        let n = next[ym*5+xm] + next[ym*5+x] + next[ym*5+xp] + next[y*5+xm] + next[y*5+xp] + next[yp*5+xm] + next[yp*5+x] + next[yp*5+xp]
        var state = 0
        if next[i] == 1
            if n == 2
                state = 1
            if n == 3
                state = 1
        if next[i] == 0
            if n == 3
                state = 1
        grid[i] = state

    gen = gen + 1

// Readback after 4 generations
let host = grid

// Census: count all live cells in the entire 5x5 grid
var census = 0
var k = 0
while k < 25
    census = census + host[k]
    k = k + 1

println(f"{census}")
"#;
    assert_gpu_runs_with_output(source, "5");
}

// Multi-pass Game of Life tests (F35 + Item 6)

/// Test the full multi-pass demo manifest.
/// Item 6: Verifies the manifest has 5 framePasses.
#[test]
fn test_multipass_game_of_life_manifest() {
    use super::helpers::compile_to_manifest;

    // The full demo should compile to a manifest with 5 frame passes
    let source = r#"
use system.collections.array

fn main()
    gpu var grid_a = Array<int, 16>()
    gpu var grid_b = Array<int, 16>()
    gpu var trail_a = Array<f32, 16>()
    gpu var trail_b = Array<f32, 16>()
    gpu var paint = Array<f32, 64>()

    gpu forall idx in 0..16
        let hash = (idx * 37) % 5
        grid_a[idx] = 1 if hash < 2 else 0
        trail_a[idx] = 0.0

    gpu frame
        gpu forall idx in 0..16
            grid_b[idx] = grid_a[idx]
        gpu forall idx in 0..16
            trail_b[idx] = trail_a[idx] * 0.9
        gpu forall idx in 0..16
            if frame.mouse_down
                grid_b[idx] = 1
        gpu forall idx in 0..16
            if frame.double_clicked
                grid_b[idx] = 0
        gpu forall idx in 0..16
            let alive = grid_b[idx]
            let base = idx * 4
            paint[base] = 1.0 if alive == 1 else 0.0

    let host = grid_b
    var k = 0
    while k < 16
        k = k + 1
"#;
    let manifest = compile_to_manifest(source).expect("manifest");
    let frame_passes = &manifest["framePasses"];
    assert!(frame_passes.is_array(), "framePasses must be an array");
    assert_eq!(
        frame_passes.as_array().unwrap().len(),
        5,
        "5-pass block should have 5 framePasses"
    );
}
