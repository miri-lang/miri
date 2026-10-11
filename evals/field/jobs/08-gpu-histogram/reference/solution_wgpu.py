#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""A Python and wgpu solution to the histogram job.

Written to prove the job can be won by a baseline arm: one compute dispatch
generates every value and adds it to its bin with an atomic add. Run with
`uv run --with wgpu`.
"""

import struct
import sys

import wgpu

SHADER = """
struct Job { count: u32, seed: u32, bins: u32, pad: u32 }
@group(0) @binding(0) var<uniform> job: Job;
@group(0) @binding(1) var<storage, read_write> hist: array<atomic<u32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= job.count) { return; }
    let a = ((i % 40000u) * 1597u + job.seed + 51749u) % 40000u;
    let b = (a * a + a * 37u + 11u) % 40009u;
    let c = (b * 613u + 7919u) % 40009u;
    let d = (c + (i / 40000u) * 7919u) % 40009u;
    atomicAdd(&hist[(d % 1000u) * job.bins / 1000u], 1u);
}
"""


def histogram(count, seed, bins):
    device = wgpu.utils.get_default_device()
    job = device.create_buffer_with_data(data=struct.pack("4I", count, seed, bins, 0), usage=wgpu.BufferUsage.UNIFORM)
    hist = device.create_buffer(size=4 * bins, usage=wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC)
    pipeline = device.create_compute_pipeline(
        layout="auto", compute={"module": device.create_shader_module(code=SHADER), "entry_point": "main"}
    )
    group = device.create_bind_group(
        layout=pipeline.get_bind_group_layout(0),
        entries=[{"binding": 0, "resource": {"buffer": job}}, {"binding": 1, "resource": {"buffer": hist}}],
    )
    encoder = device.create_command_encoder()
    if count:
        compute = encoder.begin_compute_pass()
        compute.set_pipeline(pipeline)
        compute.set_bind_group(0, group)
        compute.dispatch_workgroups((count + 255) // 256)
        compute.end()
    device.queue.submit([encoder.finish()])
    return list(device.queue.read_buffer(hist).cast("I"))


def main():
    count, seed, bins = (int(field) for field in open(sys.argv[1], encoding="utf-8").read().split())
    for index, number in enumerate(histogram(count, seed, bins)):
        print(f"BIN {index} {number}")
    print(f"TOTAL {count}")


main()
