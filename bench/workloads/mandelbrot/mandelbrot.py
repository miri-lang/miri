# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=657a14ccacfbf462c39b7575ed9a6dc74dd107112be42ca770daedfdac1e97e7

"""mandelbrot: the Mandelbrot set packed as a bitmap (see mandelbrot.mi)."""

import sys
import time


def stays_bounded(cr, ci):
    zr = 0.0
    zi = 0.0
    for _ in range(50):
        tr = zr * zr - zi * zi + cr
        zi = 2.0 * zr * zi + ci
        zr = tr
        if zr * zr + zi * zi > 4.0:
            return False
    return True


def kernel(n):
    h = 0
    inside = 0
    for y in range(n):
        ci = 2.0 * y / n - 1.0
        byte = 0
        bits = 0
        for x in range(n):
            cr = 2.0 * x / n - 1.5
            byte = byte * 2
            if stays_bounded(cr, ci):
                byte += 1
                inside += 1
            bits += 1
            if bits == 8:
                h = (h * 31 + byte) % 1000000007
                byte = 0
                bits = 0
        if bits > 0:
            byte = byte << (8 - bits)
            h = (h * 31 + byte) % 1000000007
    return f"{h} {inside}"


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
