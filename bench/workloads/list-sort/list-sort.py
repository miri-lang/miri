# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=f2b5f559fd365dbce888a810b2c008b01a83163d9380fdd7126ab0effa775f62

"""list-sort: sort generated numbers, then records by key (see list-sort.mi)."""

import sys
import time


def kernel(n):
    numbers = [0] * n
    records = [(0, 0)] * n
    state = 7
    for i in range(n):
        state = state * 48271 % 2147483647
        numbers[i] = state % 1000000
        records[i] = (state % 1000, i)
    numbers.sort()
    by_key = sorted(records, key=lambda record: (record[0], -record[1]))
    number_hash = 0
    record_hash = 0
    for i in range(n):
        number_hash = (number_hash * 31 + numbers[i]) % 1000000007
        record_hash = (record_hash * 31 + by_key[i][1]) % 1000000007
    return f"{number_hash} {record_hash}"


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
