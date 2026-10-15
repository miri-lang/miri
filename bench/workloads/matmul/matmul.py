# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=06be70f09f410ba30a7fe974d4dccc6f5d664e06f068fec17b5a6e01b2b70e0c

"""matmul: the naive triple-loop matrix product (see matmul.mi)."""

import sys
import time


def matrix(n, offset):
    return [[float((i * j + offset * (i + j)) % 10) for j in range(n)] for i in range(n)]


def kernel(n):
    a = matrix(n, 1)
    b = matrix(n, 2)
    total = 0.0
    for i in range(n):
        row = a[i]
        for j in range(n):
            s = 0.0
            for k in range(n):
                s = s + row[k] * b[k][j]
            total = total + s
    return f"{int(total)}"


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
