# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=136dbd98535d5fda64543e9fe3d501207e7462585ae049721db3dfae06846521

"""fannkuch-redux: pancake flips over every permutation (see fannkuch-redux.mi)."""

import sys
import time


def count_flips(perm, p):
    p[:] = perm
    flips = 0
    first = p[0]
    while first != 0:
        p[: first + 1] = p[first::-1]
        flips += 1
        first = p[0]
    return flips


def kernel(n):
    perm = list(range(n))
    count = [0] * n
    scratch = [0] * n
    checksum = 0
    max_flips = 0
    index = 0
    while True:
        flips = count_flips(perm, scratch)
        max_flips = max(max_flips, flips)
        checksum += flips if index % 2 == 0 else -flips
        index += 1
        r = 1
        while r < n:
            perm.insert(r, perm.pop(0))
            count[r] += 1
            if count[r] <= r:
                break
            count[r] = 0
            r += 1
        else:
            return f"{checksum} {max_flips}"


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
