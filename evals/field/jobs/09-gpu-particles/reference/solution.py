#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the particles job.

It computes on the processor, in double precision, on purpose. The job asks a
subject for GPU kernels, which compute in single precision; the job's tolerance
absorbs that, and the expectations stay the exact answer rather than one
device's rounding.
"""

import math
import sys


def read_particles(path):
    lines = open(path, "rb").read().decode("utf-8").strip().split("\n")
    count, steps, dt = lines[0].split()
    particles = [[float(field) for field in line.split()] for line in lines[1:1 + int(count)]]
    return int(steps), float(dt), particles


def step(particles, dt):
    accelerations = []
    for i, (xi, yi, _, _) in enumerate(particles):
        ax = ay = 0.0
        for j, (xj, yj, _, _) in enumerate(particles):
            if i != j:
                dx, dy = xj - xi, yj - yi
                scale = (dx * dx + dy * dy + 0.01) ** -1.5
                ax += dx * scale
                ay += dy * scale
        accelerations.append((ax, ay))
    moved = []
    for (x, y, vx, vy), (ax, ay) in zip(particles, accelerations):
        vx, vy = vx + ax * dt, vy + ay * dt
        moved.append([x + vx * dt, y + vy * dt, vx, vy])
    return moved


def main():
    steps, dt, particles = read_particles(sys.argv[1])
    for _ in range(steps):
        particles = step(particles, dt)
    count = len(particles)
    cx = sum(p[0] for p in particles) / count
    cy = sum(p[1] for p in particles) / count
    spread = math.sqrt(sum((p[0] - cx) ** 2 + (p[1] - cy) ** 2 for p in particles) / count)
    print(f"CENTER {cx:.6f} {cy:.6f}")
    print(f"MOMENTUM {sum(p[2] for p in particles):.6f} {sum(p[3] for p in particles):.6f}")
    print(f"KINETIC {sum((p[2] ** 2 + p[3] ** 2) / 2 for p in particles):.6f}")
    print(f"SPREAD {spread:.6f}")


main()
