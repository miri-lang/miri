// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Device programs handing `out` arguments over, lowered through every MIR
//! pass with the verifier on: the kernel and every device helper it reaches
//! hand each `out` argument over as a whole temp, and carry no count.
//!
//! A device helper is cloned from the body lowered for the host, which asks
//! whether a collection it writes an element of is shared and copies it on
//! write. Device code holds no counts, so the clone asks nothing.

use crate::mir::utils::out_handovers;
use miri::mir::verify::verify_out_places;
use miri::mir::Body;
use miri::pipeline::Pipeline;
use miri::type_checker::utils::ResourceTypes;

const HELPERS: &str = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn bump(x out int)
    x = x + 1

fn bumpf(x out f32)
    x = x + 1.0
";

/// The GPU bodies of `source`, kernels and device helpers, lowered through
/// every MIR pass with the verifier on, panicking on any refusal, and on any
/// way of handing an `out` argument over or of asking about a count that
/// device code breaks; the backend's own verifier holds those to the bodies
/// it receives, which a native build of a GPU program reaches only once its
/// kernels emit.
fn verified_gpu_bodies(source: &str) -> Vec<Body> {
    let program = Pipeline::new()
        .with_verify_mir(true)
        .get_gpu_program_in(source, false)
        .unwrap_or_else(|error| panic!("lowering or verification failed: {error:?}"));
    let bodies: Vec<Body> = (program.bodies.into_iter())
        .map(|(_, body)| body)
        .filter(Body::is_gpu)
        .collect();
    for body in &bodies {
        let violations = verify_out_places(body, &ResourceTypes::default());
        assert!(violations.is_empty(), "{violations:?} in\n{body}");
    }
    bodies
}

/// Asserts `source` lowers and verifies, and that its GPU bodies hand over
/// `expected` `out` arguments, each a whole temp.
fn assert_handed_over_whole(source: &str, expected: usize) {
    let bodies = verified_gpu_bodies(source);
    let handovers: Vec<_> = bodies.iter().flat_map(out_handovers).collect();
    assert_eq!(handovers.len(), expected, "out arguments in {bodies:#?}");
    for handover in &handovers {
        assert!(
            handover.operand.projection.is_empty(),
            "an out operand in device code is a whole temp, not `{}`",
            handover.operand
        );
    }
}

#[test]
fn a_whole_local_handed_out_in_a_pass_verifies() {
    assert_handed_over_whole(
        &format!(
            "{HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var t = i
        bump(t)
        dst[i] = t
"
        ),
        1,
    );
}

#[test]
fn a_buffer_element_handed_out_in_a_pass_verifies() {
    assert_handed_over_whole(
        &format!(
            "{HELPERS}
fn main()
    gpu var dst = [10, 20, 30, 40]
    gpu forall i in 0..4
        bump(dst[i])
"
        ),
        1,
    );
}

#[test]
fn vector_components_handed_out_in_a_pass_verify() {
    assert_handed_over_whole(
        &format!(
            "{HELPERS}
fn main()
    gpu var pts = [Vec2<f32>(1.0, 2.0), Vec2<f32>(3.0, 4.0)]
    gpu forall i in 0..2
        var v = Vec2<f32>(1.0, 2.0)
        bumpf(v.x)
        bumpf(pts[i].x)
        pts[i].y = v.x
"
        ),
        2,
    );
}

#[test]
fn a_helper_handing_out_an_element_of_its_own_array_verifies() {
    assert_handed_over_whole(
        &format!(
            "{HELPERS}
fn helper(k int) int
    var a = [k, k]
    bump(a[0])
    a[0]

fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = helper(i)
"
        ),
        1,
    );
}

#[test]
fn a_helper_writing_an_element_of_its_own_array_asks_about_no_count() {
    let bodies = verified_gpu_bodies(
        "
use system.gpu
use system.collections.array

fn helper(k int) int
    var a = [k, k]
    a[0] = a[1] + 1
    a[0]

fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = helper(i)
",
    );
    assert_eq!(bodies.len(), 2, "the kernel and its helper");
}
