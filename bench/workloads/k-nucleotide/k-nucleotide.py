# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=4eb3e9770ccff621b5631d6da6d802d24402050e0176946e3768ab8ddc5ba729

"""k-nucleotide: count the substrings of a DNA sequence (see k-nucleotide.mi)."""

import sys
import time

MOTIF = "GGTATTTTAATTTATAGT"


def frequencies(sequence, k):
    counts = {}
    for i in range(len(sequence) - k + 1):
        key = sequence[i : i + k]
        counts[key] = counts.get(key, 0) + 1
    return counts


def listing(sequence, k):
    counts = frequencies(sequence, k)
    return " ".join(f"{key}={counts[key]}" for key in sorted(counts))


def kernel(text):
    sequence = text.strip()
    parts = [listing(sequence, 1), listing(sequence, 2)]
    for length in (3, 4, 6, 12, 18):
        prefix = MOTIF[:length]
        parts.append(f"{prefix}={frequencies(sequence, length).get(prefix, 0)}")
    return " ".join(parts)


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
