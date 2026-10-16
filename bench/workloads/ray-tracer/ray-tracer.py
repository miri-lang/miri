# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=8b30d92d79ccba4e1ae7456e2246ca3a7b8876a81c71f87728586abb6c1db972

"""ray-tracer: spheres shaded by a light, one ray per pixel (see ray-tracer.mi)."""

import math
import sys
import time
from typing import NamedTuple


class Vec3(NamedTuple):
    x: float
    y: float
    z: float


class Sphere(NamedTuple):
    center: Vec3
    radius: float


def sub(a, b):
    return Vec3(a.x - b.x, a.y - b.y, a.z - b.z)


def scale(a, k):
    return Vec3(a.x * k, a.y * k, a.z * k)


def dot(a, b):
    return a.x * b.x + a.y * b.y + a.z * b.z


def normalize(a):
    return scale(a, 1.0 / math.sqrt(dot(a, a)))


def hit(sphere, direction):
    oc = scale(sphere.center, -1.0)
    b = dot(oc, direction)
    c = dot(oc, oc) - sphere.radius * sphere.radius
    discriminant = b * b - c
    if discriminant < 0.0:
        return -1.0
    return -b - math.sqrt(discriminant)


def scene():
    return [
        Sphere(Vec3(0.0, 0.0, 5.0), 1.0),
        Sphere(Vec3(1.5, 0.5, 6.0), 1.0),
        Sphere(Vec3(-1.5, -0.5, 4.5), 0.75),
        Sphere(Vec3(0.0, -101.0, 5.0), 100.0),
    ]


def kernel(n):
    spheres = scene()
    light = normalize(Vec3(-1.0, 1.0, -1.0))
    h = 0
    hits = 0
    for py in range(n):
        for px in range(n):
            direction = normalize(Vec3(px / n - 0.5, 0.5 - py / n, 1.0))
            nearest = 1000000.0
            shade = 0
            for sphere in spheres:
                t = hit(sphere, direction)
                if 0.0 < t < nearest:
                    nearest = t
                    normal = normalize(sub(scale(direction, t), sphere.center))
                    lambert = dot(normal, light)
                    shade = int(lambert * 255.0) if lambert > 0.0 else 0
            if nearest < 1000000.0:
                hits += 1
            h = (h * 31 + shade) % 1000000007
    return f"{h} {hits}"


def run_hot(n):
    expected = kernel(n)
    agree = True
    for _ in range(4):
        checksum = kernel(n)
        agree = agree and checksum == expected
    for _ in range(5):
        start = time.perf_counter_ns()
        checksum = kernel(n)
        elapsed = time.perf_counter_ns() - start
        print(f"hot_ns={elapsed}", file=sys.stderr)
        agree = agree and checksum == expected
    if not agree:
        print("checksum differs between iterations", file=sys.stderr)
        sys.exit(1)
    return expected


def main():
    with open(sys.argv[1]) as f:
        n = int(f.read().strip())
    hot = len(sys.argv) > 2 and sys.argv[2] == "hot"
    print(run_hot(n) if hot else kernel(n))


if __name__ == "__main__":
    main()
