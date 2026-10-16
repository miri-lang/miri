# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=3ff8d82e6d8ce780dfaebcd67777305286719d1c10bf335663a91118b3fd3232

"""wordfreq: count the words of a text (see wordfreq.mi)."""

import sys
import time


def kernel(text):
    counts = {}
    for line in text.split("\n"):
        for word in line.split(" "):
            if word != "":
                counts[word] = counts.get(word, 0) + 1
    ranked = sorted(counts.items(), key=lambda entry: (-entry[1], entry[0]))
    top = ",".join(f"{word}={count}" for word, count in ranked[:10])
    return f"{len(counts)} {top}"


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
