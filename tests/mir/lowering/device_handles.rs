// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lower_code;
use miri::mir::Body;

const GPU_SOURCE: &str = "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
";

/// The device handle of every gpu-resident local, in declaration order.
fn device_handles(body: &Body) -> Vec<u64> {
    body.local_decls
        .iter()
        .filter_map(|decl| decl.device_handle.map(|handle| handle.0))
        .collect()
}

/// Lowering the same source twice in one process must assign the same device
/// handles both times.
///
/// The ids are emitted into the generated code, so drawing them from a counter
/// that outlives one compilation makes a long-lived compiler host — the agent
/// session, a watch loop, an IDE backend — produce different machine code for
/// identical source depending on how much it compiled beforehand.
#[test]
fn repeated_lowering_assigns_the_same_device_handles() {
    let first = device_handles(&mir_lower_code(GPU_SOURCE));
    let second = device_handles(&mir_lower_code(GPU_SOURCE));

    assert!(
        !first.is_empty(),
        "the fixture must allocate at least one device handle"
    );
    assert_eq!(
        first, second,
        "device handles drifted between two lowerings of identical source"
    );
}
