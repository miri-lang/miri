# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=614d7e7535c714ab824552dbe38bedb6557e348d534cc8e4f05fdc11237c0697

"""generics: one generic stack at three element types (see generics.mi)."""

import sys
import time
from dataclasses import dataclass
from typing import Generic, TypeVar

T = TypeVar("T")
A = TypeVar("A")
B = TypeVar("B")


class Stack(Generic[T]):
    def __init__(self):
        self.items: list[T] = []

    def push(self, item: T):
        self.items.append(item)

    def pop(self) -> T:
        return self.items.pop()

    def size(self) -> int:
        return len(self.items)


@dataclass
class Pair(Generic[A, B]):
    first: A
    second: B


def kernel(n):
    ints: Stack[int] = Stack()
    floats: Stack[float] = Stack()
    pairs: Stack[Pair[int, float]] = Stack()
    for i in range(n):
        ints.push(i % 1000)
        floats.push(float(i % 100) * 0.5)
        pairs.push(Pair(i % 10, float(i % 4)))
    a = 0
    while ints.size() > 0:
        a = (a * 31 + ints.pop()) % 1000000007
    b = 0.0
    while floats.size() > 0:
        b = b + floats.pop()
    c = 0
    while pairs.size() > 0:
        p = pairs.pop()
        c = (c * 7 + p.first + int(p.second)) % 1000000007
    return f"{a} {int(b)} {c}"


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
