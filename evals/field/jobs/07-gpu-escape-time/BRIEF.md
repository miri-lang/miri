# Job — Escape times on the GPU

Write a program in {{LANGUAGE}} that computes Mandelbrot escape times over a
grid of points, computing every point on the GPU, and summarizes them.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

The first command-line argument is the path of a text file of two lines:

```
<width> <height> <limit>
<x_min> <x_max> <y_min> <y_max>
```

`width` and `height` are whole numbers between 1 and 1024, and `limit` between 1
and 1000. The four bounds are decimal numbers with `x_min < x_max` and
`y_min < y_max`.

The grid has `width` columns and `height` rows. The point of column `i` and row
`j`, counting from 0, is

```
cx = x_min + (i + 0.5) * (x_max - x_min) / width
cy = y_min + (j + 0.5) * (y_max - y_min) / height
```

Its **escape time** is the number of steps of this loop, starting from
`zx = zy = 0` and `n = 0`:

```
while n < limit and zx * zx + zy * zy <= 4:
    zx, zy = zx * zx - zy * zy + cx, 2 * zx * zy + cy
    n = n + 1
```

A point whose escape time equals `limit` is **inside**; every other point has
**escaped**.

Write ten lines to standard output, in this order:

```
INSIDE <the number of inside points>
BIN 0 <count>
...
BIN 7 <count>
MEAN <the mean escape time of the escaped points>
```

An escaped point with escape time `n` belongs to bin `floor(8 * n / limit)`, so
the eight `BIN` lines count every escaped point once. `MEAN` is written with
three digits after the decimal point, and is `0.000` when no point escaped.

Single-precision arithmetic is fine. A point near the boundary of the set may
land in a neighbouring bin, and the totals are checked with a small tolerance
that allows for it.

The per-point computation must be done by a kernel the GPU runs. A loop over the
points on the processor is not this job, even though it would produce the same
numbers.

Write nothing else to standard output, and exit with status 0.

## Done

The job is finished when the program builds, computes the escape times on the
GPU, and satisfies the contract for any input, not only the one you tried. There
are no tests in the directory; the contract above is the specification.
