#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""A Python and wgpu solution to the escape-time job.

Written to prove the job can be won by a baseline arm: one compute dispatch
writes every point's escape time, and the host folds them into the summary.
Run with `uv run --with wgpu`.
"""

import struct
import sys

import wgpu

SHADER = """
struct Job { width: u32, height: u32, limit: u32, pad: u32, x_min: f32, x_max: f32, y_min: f32, y_max: f32 }
@group(0) @binding(0) var<uniform> job: Job;
@group(0) @binding(1) var<storage, read_write> times: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= job.width * job.height) { return; }
    let cx = job.x_min + (f32(id.x % job.width) + 0.5) * (job.x_max - job.x_min) / f32(job.width);
    let cy = job.y_min + (f32(id.x / job.width) + 0.5) * (job.y_max - job.y_min) / f32(job.height);
    var zx = 0.0;
    var zy = 0.0;
    var n = 0u;
    while (n < job.limit && zx * zx + zy * zy <= 4.0) {
        let next_x = zx * zx - zy * zy + cx;
        zy = 2.0 * zx * zy + cy;
        zx = next_x;
        n = n + 1u;
    }
    times[id.x] = n;
}
"""


def read_job(path):
    lines = open(path, encoding="utf-8").read().strip().split("\n")
    width, height, limit = (int(field) for field in lines[0].split())
    bounds = [float(field) for field in lines[1].split()]
    return width, height, limit, bounds


def escape_times(width, height, limit, bounds):
    device = wgpu.utils.get_default_device()
    count = width * height
    job = device.create_buffer_with_data(
        data=struct.pack("4I4f", width, height, limit, 0, *bounds), usage=wgpu.BufferUsage.UNIFORM
    )
    times = device.create_buffer(size=4 * count, usage=wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC)
    pipeline = device.create_compute_pipeline(
        layout="auto", compute={"module": device.create_shader_module(code=SHADER), "entry_point": "main"}
    )
    group = device.create_bind_group(
        layout=pipeline.get_bind_group_layout(0),
        entries=[{"binding": 0, "resource": {"buffer": job}}, {"binding": 1, "resource": {"buffer": times}}],
    )
    encoder = device.create_command_encoder()
    compute = encoder.begin_compute_pass()
    compute.set_pipeline(pipeline)
    compute.set_bind_group(0, group)
    compute.dispatch_workgroups((count + 63) // 64)
    compute.end()
    device.queue.submit([encoder.finish()])
    return list(device.queue.read_buffer(times).cast("I"))


def main():
    width, height, limit, bounds = read_job(sys.argv[1])
    times = escape_times(width, height, limit, bounds)
    bins = [0] * 8
    escaped = [n for n in times if n < limit]
    for n in escaped:
        bins[8 * n // limit] += 1
    print(f"INSIDE {len(times) - len(escaped)}")
    for index, count in enumerate(bins):
        print(f"BIN {index} {count}")
    mean = sum(escaped) / len(escaped) if escaped else 0.0
    print(f"MEAN {mean:.3f}")


main()
