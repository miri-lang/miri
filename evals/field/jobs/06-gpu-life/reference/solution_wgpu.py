#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""A Python and wgpu solution to the Game of Life job.

Written to prove the job can be won by a baseline arm: one compute dispatch per
generation, ping-ponging two storage buffers. Run with `uv run --with wgpu`.
"""

import struct
import sys

import wgpu

SHADER = """
struct Size { width: u32, height: u32 }
@group(0) @binding(0) var<uniform> size: Size;
@group(0) @binding(1) var<storage, read> current: array<u32>;
@group(0) @binding(2) var<storage, read_write> next: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = size.width * size.height;
    if (id.x >= n) { return; }
    let x = i32(id.x % size.width);
    let y = i32(id.x / size.width);
    var live = 0u;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let nx = x + dx;
            let ny = y + dy;
            if ((dx != 0 || dy != 0) && nx >= 0 && nx < i32(size.width) && ny >= 0 && ny < i32(size.height)) {
                live += current[u32(ny) * size.width + u32(nx)];
            }
        }
    }
    let alive = current[id.x] == 1u;
    next[id.x] = select(0u, 1u, live == 3u || (live == 2u && alive));
}
"""


def read_grid(path):
    lines = open(path, encoding="utf-8").read().strip().split("\n")
    width, height, generations = (int(field) for field in lines[0].split())
    cells = [int(cell) for row in range(height) for cell in lines[1 + row].split()]
    return width, height, generations, cells


def run(width, height, generations, cells):
    device = wgpu.utils.get_default_device()
    usage = wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC | wgpu.BufferUsage.COPY_DST
    data = struct.pack(f"{len(cells)}I", *cells)
    buffers = [device.create_buffer_with_data(data=data, usage=usage), device.create_buffer(size=len(data), usage=usage)]
    size = device.create_buffer_with_data(data=struct.pack("II", width, height), usage=wgpu.BufferUsage.UNIFORM)
    pipeline = device.create_compute_pipeline(
        layout="auto", compute={"module": device.create_shader_module(code=SHADER), "entry_point": "main"}
    )
    groups = [
        device.create_bind_group(
            layout=pipeline.get_bind_group_layout(0),
            entries=[
                {"binding": 0, "resource": {"buffer": size}},
                {"binding": 1, "resource": {"buffer": buffers[source]}},
                {"binding": 2, "resource": {"buffer": buffers[1 - source]}},
            ],
        )
        for source in (0, 1)
    ]
    encoder = device.create_command_encoder()
    for generation in range(generations):
        compute = encoder.begin_compute_pass()
        compute.set_pipeline(pipeline)
        compute.set_bind_group(0, groups[generation % 2])
        compute.dispatch_workgroups((len(cells) + 63) // 64)
        compute.end()
    device.queue.submit([encoder.finish()])
    result = device.queue.read_buffer(buffers[generations % 2]).cast("I")
    return list(result)


def main():
    width, height, generations, cells = read_grid(sys.argv[1])
    cells = run(width, height, generations, cells)
    for row in range(height):
        print(" ".join(str(cell) for cell in cells[row * width:(row + 1) * width]))
    print(f"ALIVE {sum(cells)}")


main()
