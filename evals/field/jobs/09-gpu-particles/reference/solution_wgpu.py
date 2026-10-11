#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""A Python and wgpu solution to the particles job.

Written to prove the job can be won by a baseline arm: each step is two compute
dispatches, one for every acceleration and one to integrate, over a buffer of
`vec4<f32>` particles. Run with `uv run --with wgpu`.
"""

import math
import struct
import sys

import wgpu

SHADER = """
struct Job { count: u32, pad0: u32, pad1: u32, dt: f32 }
@group(0) @binding(0) var<uniform> job: Job;
@group(0) @binding(1) var<storage, read_write> particles: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> accelerations: array<vec2<f32>>;

@compute @workgroup_size(64)
fn accelerate(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= job.count) { return; }
    var sum = vec2<f32>(0.0, 0.0);
    for (var j = 0u; j < job.count; j++) {
        if (j != i) {
            let d = particles[j].xy - particles[i].xy;
            let d2 = dot(d, d) + 0.01;
            sum += d / (d2 * sqrt(d2));
        }
    }
    accelerations[i] = sum;
}

@compute @workgroup_size(64)
fn integrate(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= job.count) { return; }
    var p = particles[i];
    let v = p.zw + accelerations[i] * job.dt;
    particles[i] = vec4<f32>(p.xy + v * job.dt, v);
}
"""


def read_particles(path):
    lines = open(path, encoding="utf-8").read().strip().split("\n")
    count, steps, dt = lines[0].split()
    particles = [float(field) for line in lines[1:1 + int(count)] for field in line.split()]
    return int(count), int(steps), float(dt), particles


def simulate(count, steps, dt, particles):
    device = wgpu.utils.get_default_device()
    job = device.create_buffer_with_data(data=struct.pack("3If", count, 0, 0, dt), usage=wgpu.BufferUsage.UNIFORM)
    state = device.create_buffer_with_data(
        data=struct.pack(f"{len(particles)}f", *particles),
        usage=wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC,
    )
    accelerations = device.create_buffer(size=8 * count, usage=wgpu.BufferUsage.STORAGE)
    module = device.create_shader_module(code=SHADER)
    layout = device.create_bind_group_layout(
        entries=[
            {"binding": 0, "visibility": wgpu.ShaderStage.COMPUTE, "buffer": {"type": wgpu.BufferBindingType.uniform}},
            {"binding": 1, "visibility": wgpu.ShaderStage.COMPUTE, "buffer": {"type": wgpu.BufferBindingType.storage}},
            {"binding": 2, "visibility": wgpu.ShaderStage.COMPUTE, "buffer": {"type": wgpu.BufferBindingType.storage}},
        ]
    )
    pipeline_layout = device.create_pipeline_layout(bind_group_layouts=[layout])
    passes = [
        device.create_compute_pipeline(layout=pipeline_layout, compute={"module": module, "entry_point": name})
        for name in ("accelerate", "integrate")
    ]
    group = device.create_bind_group(
        layout=layout,
        entries=[
            {"binding": 0, "resource": {"buffer": job}},
            {"binding": 1, "resource": {"buffer": state}},
            {"binding": 2, "resource": {"buffer": accelerations}},
        ],
    )
    encoder = device.create_command_encoder()
    for _ in range(steps):
        for pipeline in passes:
            compute = encoder.begin_compute_pass()
            compute.set_pipeline(pipeline)
            compute.set_bind_group(0, group)
            compute.dispatch_workgroups((count + 63) // 64)
            compute.end()
    device.queue.submit([encoder.finish()])
    flat = list(device.queue.read_buffer(state).cast("f"))
    return [flat[k:k + 4] for k in range(0, len(flat), 4)]


def main():
    count, steps, dt, particles = read_particles(sys.argv[1])
    moved = simulate(count, steps, dt, particles)
    cx = sum(p[0] for p in moved) / count
    cy = sum(p[1] for p in moved) / count
    spread = math.sqrt(sum((p[0] - cx) ** 2 + (p[1] - cy) ** 2 for p in moved) / count)
    print(f"CENTER {cx:.6f} {cy:.6f}")
    print(f"MOMENTUM {sum(p[2] for p in moved):.6f} {sum(p[3] for p in moved):.6f}")
    print(f"KINETIC {sum((p[2] ** 2 + p[3] ** 2) / 2 for p in moved):.6f}")
    print(f"SPREAD {spread:.6f}")


main()
