# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=eb9d4c017e0577474f1d3a5046e516e99a73daf03cbc9ab996aff7ff1971b3cf

"""string-builder: build a long string by appending (see string-builder.mi)."""

import sys
import time


def kernel(n):
    text = ""
    for i in range(n):
        text += f"{i}:{i * i % 1000};"
    h = 0
    for byte in text.encode():
        h = (h * 31 + byte) % 1000000007
    return f"{len(text)} {h}"


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
