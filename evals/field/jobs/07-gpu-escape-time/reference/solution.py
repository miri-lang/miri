#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the escape-time job.

It computes on the processor, in double precision, on purpose. A GPU kernel
computes in single precision, so a point on the boundary of the set can escape
a step earlier or later; the job's tolerance is sized to absorb that, and the
expectations stay the exact answer rather than one device's rounding.
"""

import sys


def read_job(path):
    lines = open(path, "rb").read().decode("utf-8").strip().split("\n")
    width, height, limit = (int(field) for field in lines[0].split())
    x_min, x_max, y_min, y_max = (float(field) for field in lines[1].split())
    return width, height, limit, x_min, x_max, y_min, y_max


def escape_time(cx, cy, limit):
    zx = zy = 0.0
    n = 0
    while n < limit and zx * zx + zy * zy <= 4:
        zx, zy = zx * zx - zy * zy + cx, 2 * zx * zy + cy
        n += 1
    return n


def main():
    width, height, limit, x_min, x_max, y_min, y_max = read_job(sys.argv[1])
    inside, bins, escaped = 0, [0] * 8, []
    for j in range(height):
        cy = y_min + (j + 0.5) * (y_max - y_min) / height
        for i in range(width):
            cx = x_min + (i + 0.5) * (x_max - x_min) / width
            n = escape_time(cx, cy, limit)
            if n == limit:
                inside += 1
            else:
                bins[8 * n // limit] += 1
                escaped.append(n)
    print(f"INSIDE {inside}")
    for index, count in enumerate(bins):
        print(f"BIN {index} {count}")
    mean = sum(escaped) / len(escaped) if escaped else 0.0
    print(f"MEAN {mean:.3f}")


main()
