#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the histogram job.

It computes on the processor on purpose. The job asks a subject for GPU
kernels; the expectations only need the counts those kernels must produce, and
a reference that needs a GPU adapter could not generate them on every machine.
"""

import sys


def value(i, seed):
    a = ((i % 40000) * 1597 + seed + 51749) % 40000
    b = (a * a + a * 37 + 11) % 40009
    c = (b * 613 + 7919) % 40009
    d = (c + (i // 40000) * 7919) % 40009
    return d % 1000


def main():
    count, seed, bins = (int(field) for field in open(sys.argv[1], encoding="utf-8").read().split())
    counts = [0] * bins
    for i in range(count):
        counts[value(i, seed) * bins // 1000] += 1
    for index, number in enumerate(counts):
        print(f"BIN {index} {number}")
    print(f"TOTAL {count}")


main()
