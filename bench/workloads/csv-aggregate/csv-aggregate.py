# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=c2fd96005a50b7f40a9a9d558d52296cd64f48f00f91644e44604f8b486884da

"""csv-aggregate: totals by key over a generated CSV file (see csv-aggregate.mi)."""

import sys
import time
from dataclasses import dataclass


@dataclass
class Totals:
    count: int
    quantity: int
    amount: float


def kernel(text):
    groups = {}
    for line in text.split("\n")[1:]:
        if line == "":
            continue
        fields = line.split(",")
        t = groups.get(fields[0])
        if t is None:
            t = groups[fields[0]] = Totals(0, 0, 0.0)
        t.count += 1
        t.quantity += int(fields[1])
        t.amount = t.amount + float(fields[2])
    return ",".join(f"{key}:{t.count}:{t.quantity}:{int(t.amount * 100.0)}" for key, t in sorted(groups.items()))


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
        n = f.read()
    hot = len(sys.argv) > 2 and sys.argv[2] == "hot"
    print(run_hot(n) if hot else kernel(n))


if __name__ == "__main__":
    main()
