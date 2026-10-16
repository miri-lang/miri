# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=72849e3f687e9801f25f263ac9d4edc2279e353ecf051f1bc601018375b7ee99

"""closures: capturing map, filter and fold pipelines (see closures.mi)."""

import sys
import time


def kernel(n):
    xs = list(range(n))
    total = 0
    for k in range(1, 9):
        scale = k * 7
        threshold = 100 * k
        scaled = [(x * scale) % 1000 for x in xs]
        kept = [x for x in scaled if x > threshold]
        acc = 0
        for x in kept:
            acc = (acc + x) % 1000000007
        total = (total + acc) % 1000000007
    return f"{total}"


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
