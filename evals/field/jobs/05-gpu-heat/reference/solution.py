#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the heat-diffusion job.

It computes on the processor on purpose. The job asks a subject for a GPU
kernel; the expectations only need the numbers that kernel must produce, and a
reference that needs a GPU adapter could not generate them on every machine.
"""

import sys


def read_grid(path):
    lines = open(path, "rb").read().decode("utf-8").strip().split("\n")
    width, height, steps = (int(field) for field in lines[0].split())
    grid = [[int(cell) for cell in lines[1 + row].split()] for row in range(height)]
    return width, height, steps, grid


def diffuse(width, height, grid):
    def at(row, column):
        row = min(max(row, 0), height - 1)
        column = min(max(column, 0), width - 1)
        return grid[row][column]

    return [
        [
            (4 * at(row, column) + at(row, column - 1) + at(row, column + 1) + at(row - 1, column) + at(row + 1, column)) // 8
            for column in range(width)
        ]
        for row in range(height)
    ]


def main():
    width, height, steps, grid = read_grid(sys.argv[1])
    for _ in range(steps):
        grid = diffuse(width, height, grid)
    for row in grid:
        print(" ".join(str(cell) for cell in row))
    print(f"SUM {sum(sum(row) for row in grid)}")


main()
