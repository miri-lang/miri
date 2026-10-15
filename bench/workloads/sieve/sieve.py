# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=ff37cc122aa8e8752a9936191aab9f6392bf33b15c191316f937f8923b599e30

"""sieve: the sieve of Eratosthenes (see sieve.mi)."""

import sys
import time


def kernel(n):
    composite = [False] * (n + 1)
    count = 0
    total = 0
    for p in range(2, n + 1):
        if not composite[p]:
            count += 1
            total = (total + p) % 1000000007
            for m in range(p * p, n + 1, p):
                composite[m] = True
    return f"{count} {total}"


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
