// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::compute::*;

#[test]
fn dispatch_size_round_trips_components() {
    let dispatch = DispatchSize { x: 4, y: 2, z: 1 };
    assert_eq!(dispatch.x, 4);
    assert_eq!(dispatch.y, 2);
    assert_eq!(dispatch.z, 1);
}

#[test]
fn dispatch_size_default_is_unit() {
    let d = DispatchSize::default();
    assert_eq!((d.x, d.y, d.z), (1, 1, 1));
}
