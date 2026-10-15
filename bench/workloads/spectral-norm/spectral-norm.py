# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=0e1d55ff93056259c89b2a505e85a1762ca00bd4f09c3c2a37394be2f8d64960

"""spectral-norm: power iteration on an infinite matrix (see spectral-norm.mi)."""

import math
import sys
import time


def entry(i, j):
    return 1.0 / ((i + j) * (i + j + 1) // 2 + i + 1)


def times(v):
    n = len(v)
    out = [0.0] * n
    for i in range(n):
        total = 0.0
        for j in range(n):
            total = total + entry(i, j) * v[j]
        out[i] = total
    return out


def times_transposed(v):
    n = len(v)
    out = [0.0] * n
    for i in range(n):
        total = 0.0
        for j in range(n):
            total = total + entry(j, i) * v[j]
        out[i] = total
    return out


def times_both(v):
    return times_transposed(times(v))


def kernel(n):
    u = [1.0] * n
    v = []
    for _ in range(10):
        v = times_both(u)
        u = times_both(v)
    vbv = 0.0
    vv = 0.0
    for i in range(n):
        vbv = vbv + u[i] * v[i]
        vv = vv + v[i] * v[i]
    return f"{int(math.sqrt(vbv / vv) * 1000000000.0)}"


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
