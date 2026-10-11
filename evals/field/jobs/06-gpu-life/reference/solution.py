#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the Game of Life job.

It computes on the processor on purpose. The job asks a subject for a GPU
kernel; the expectations only need the grid that kernel must produce, and a
reference that needs a GPU adapter could not generate them on every machine.
"""

import sys


def read_grid(path):
    lines = open(path, "rb").read().decode("utf-8").strip().split("\n")
    width, height, generations = (int(field) for field in lines[0].split())
    grid = [[int(cell) for cell in lines[1 + row].split()] for row in range(height)]
    return width, height, generations, grid


def step(width, height, grid):
    def alive(row, column):
        inside = 0 <= row < height and 0 <= column < width
        return grid[row][column] if inside else 0

    def next_state(row, column):
        neighbours = sum(
            alive(row + dr, column + dc)
            for dr in (-1, 0, 1)
            for dc in (-1, 0, 1)
            if (dr, dc) != (0, 0)
        )
        if grid[row][column]:
            return 1 if neighbours in (2, 3) else 0
        return 1 if neighbours == 3 else 0

    return [[next_state(row, column) for column in range(width)] for row in range(height)]


def main():
    width, height, generations, grid = read_grid(sys.argv[1])
    for _ in range(generations):
        grid = step(width, height, grid)
    for row in grid:
        print(" ".join(str(cell) for cell in row))
    print(f"ALIVE {sum(sum(row) for row in grid)}")


main()
