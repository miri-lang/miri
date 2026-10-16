# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=cd495649975c8840d41e8b762e052387d1f00ad84b3040d502694ce14a1cb87a

"""binary-trees: allocate and walk many complete binary trees (see binary-trees.mi)."""

import sys
import time


class Node:
    def __init__(self, left, right):
        self.left = left
        self.right = right


def bottom_up(depth):
    if depth == 0:
        return Node(None, None)
    return Node(bottom_up(depth - 1), bottom_up(depth - 1))


def check(node):
    if node.left is None or node.right is None:
        return 1
    return 1 + check(node.left) + check(node.right)


def kernel(max_depth):
    stretch = check(bottom_up(max_depth + 1))
    long_lived = bottom_up(max_depth)
    total = 0
    for depth in range(4, max_depth + 1, 2):
        for _ in range(2 ** (max_depth - depth + 4)):
            total += check(bottom_up(depth))
    return f"{stretch} {total} {check(long_lived)}"


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
