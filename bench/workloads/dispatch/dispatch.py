# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=cf92aafcf3c6a39fa52be5441e82b46b7cd74644451133b9912ad5356bd3d52c

"""dispatch: shapes behind one interface, their areas summed (see dispatch.mi)."""

import sys
import time


class Shape:
    def area(self):
        raise NotImplementedError


class Circle(Shape):
    def __init__(self, radius):
        self.radius = radius

    def area(self):
        return 3.0 * self.radius * self.radius


class Square(Shape):
    def __init__(self, side):
        self.side = side

    def area(self):
        return self.side * self.side


class Rectangle(Shape):
    def __init__(self, width, height):
        self.width = width
        self.height = height

    def area(self):
        return self.width * self.height


class Triangle(Shape):
    def __init__(self, base, height):
        self.base = base
        self.height = height

    def area(self):
        return self.base * self.height * 0.5


def shape(i):
    a = float(i % 7 + 1)
    b = float(i % 5 + 1)
    kind = i % 4
    if kind == 0:
        return Circle(a)
    if kind == 1:
        return Square(a)
    if kind == 2:
        return Rectangle(a, b)
    return Triangle(a, b)


def kernel(n):
    shapes = [shape(i) for i in range(1000)]
    total = 0.0
    for _ in range(n):
        for s in shapes:
            total = total + s.area()
    return f"{int(total)}"


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
