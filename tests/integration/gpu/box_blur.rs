// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Box blur correctness tests.
//!
//! These tests verify the correctness of 3×3 zero-padded box blur convolution
//! on GPU. Out-of-bounds neighbors contribute 0 to the blur, naturally darkening
//! the edges—a standard approach in digital image processing.

use super::device::assert_gpu_runs_with_output;

/// Test 3×3 box blur with a single bright pixel on a dark grid.
///
/// A single white pixel (value 1.0) on an otherwise-black grid should produce:
/// - The center pixel and all adjacent pixels: 1/9 ≈ 0.1111 (because zero-padding
///   means the 3×3 neighborhood always contains exactly 1 copy of the white pixel)
/// - Far-away pixels: 0.0 (no contribution from single white pixel)
///
/// This tests that zero-padding preserves the value: a single pixel distributed
/// across its 3×3 neighborhood (itself + 8 neighbors, of which at least 1 is the
/// white pixel and others are 0).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_single_pixel_blur() {
    let source = r#"
use system.collections.array

// Initialize a 5×5 grid with a single white pixel at center (2, 2)
gpu var src = Array<f32, 25>()
gpu forall idx in 0..25
    if idx == 2 * 5 + 2
        src[idx] = 1.0 as f32

// Apply 3×3 box blur
gpu var dst = Array<f32, 25>()
gpu forall idx in 0..25
    let y = idx / 5
    let x = idx - y * 5
    var sum = 0.0 as f32
    var dy = -1
    while dy <= 1
        var dx = -1
        while dx <= 1
            let nx = x + dx
            let ny = y + dy
            if nx >= 0
                if nx < 5
                    if ny >= 0
                        if ny < 5
                            sum = sum + src[ny * 5 + nx]
            dx = dx + 1
        dy = dy + 1
    dst[idx] = sum / (9.0 as f32)

let host = dst

// Verify center pixel (2,2): 1/9 (only itself in its 3×3 neighborhood)
// Verify an adjacent pixel (2,3): 1/9 (white pixel is in its 3×3 neighborhood)
// Verify a far pixel (0,0): 0.0 (white pixel not in its 3×3 neighborhood)
println(f"center={host[2 * 5 + 2]} adjacent={host[3 * 5 + 2]} far={host[0]}")
"#;

    // center = 1/9, adjacent = 1/9, far = 0
    assert_gpu_runs_with_output(
        source,
        "center=0.1111111119389534 adjacent=0.1111111119389534 far=0.0",
    );
}

/// Test 3×3 box blur with a 3×3 block of white pixels.
///
/// A 3×3 block of white pixels should produce:
/// - Interior pixels: 1.0 (surrounded by 9 white pixels: 9/9)
/// - Edge pixels: 6/9 ≈ 0.667 (6 white neighbors in the 3×3)
/// - Corner pixels: 4/9 ≈ 0.444 (4 white neighbors at the corners)
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_block_blur() {
    let source = r#"
use system.collections.array

// Initialize a 5×5 grid with a 3×3 white block at (1,1) to (3,3)
gpu var src = Array<f32, 25>()
gpu forall idx in 0..25
    let y = idx / 5
    let x = idx - y * 5
    var v = 0.0 as f32
    if x >= 1
        if x < 4
            if y >= 1
                if y < 4
                    v = 1.0 as f32
    src[idx] = v

// Apply 3×3 box blur
gpu var dst = Array<f32, 25>()
gpu forall idx in 0..25
    let y = idx / 5
    let x = idx - y * 5
    var sum = 0.0 as f32
    var dy = -1
    while dy <= 1
        var dx = -1
        while dx <= 1
            let nx = x + dx
            let ny = y + dy
            if nx >= 0
                if nx < 5
                    if ny >= 0
                        if ny < 5
                            sum = sum + src[ny * 5 + nx]
            dx = dx + 1
        dy = dy + 1
    dst[idx] = sum / (9.0 as f32)

let host = dst

// Verify interior pixel (2,2): 1.0 (full 3×3 of white)
// Verify edge pixel (1,2): 6/9 (top-left, top, top-right, center-left, center, center-right)
// Verify corner pixel (1,1): 4/9 (out-of-bounds top-left, top, left clamped to block edge; only 4 white in box)
println(f"interior={host[2 * 5 + 2]} edge={host[2 * 5 + 1]} corner={host[1 * 5 + 1]}")
"#;

    // interior = 9/9, edge = 6/9, corner = 4/9
    assert_gpu_runs_with_output(
        source,
        "interior=1.0 edge=0.6666666865348816 corner=0.4444444477558136",
    );
}

/// Test zero-padding behavior at boundary pixels.
///
/// A white pixel at the edge of a grid should demonstrate zero-padding: pixels
/// outside the valid domain contribute 0 to the blur, darkening the edges.
/// This verifies the blur correctly handles boundary conditions.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_zero_padding_at_edges() {
    let source = r#"
use system.collections.array

// Initialize a 3×3 grid with white pixels everywhere
gpu var src = Array<f32, 9>()
gpu forall idx in 0..9
    src[idx] = 1.0 as f32

// Apply 3×3 box blur
gpu var dst = Array<f32, 9>()
gpu forall idx in 0..9
    let y = idx / 3
    let x = idx - y * 3
    var sum = 0.0 as f32
    var dy = -1
    while dy <= 1
        var dx = -1
        while dx <= 1
            let nx = x + dx
            let ny = y + dy
            if nx >= 0
                if nx < 3
                    if ny >= 0
                        if ny < 3
                            sum = sum + src[ny * 3 + nx]
            dx = dx + 1
        dy = dy + 1
    dst[idx] = sum / (9.0 as f32)

let host = dst

// With zero-padding on a 3×3 all-white grid:
// - Interior pixel (1,1): 1.0 (full 3×3 valid: 9/9)
// - Edge pixel (0,1): 6/9 (top-left,top,top-right out of bounds; 6 of 9)
// - Corner pixel (0,0): 4/9 (top-left,top,left out of bounds; 4 of 9)
println(f"interior={host[4]} edge={host[1]} corner={host[0]}")
"#;

    // interior = 9/9 = 1.0, edge = 6/9, corner = 4/9
    assert_gpu_runs_with_output(
        source,
        "interior=1.0 edge=0.6666666865348816 corner=0.4444444477558136",
    );
}
