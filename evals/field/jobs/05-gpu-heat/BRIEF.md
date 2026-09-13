# Job — Heat diffusion on the GPU

Write a program in {{LANGUAGE}} that diffuses heat across a grid, computing the
stencil on the GPU.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

The first command-line argument is the path of a text file:

```
<width> <height> <steps>
<width integers>          <- the first row
...                       <- height rows in all
```

`width` and `height` are between 1 and 512; `steps` is between 0 and 1000. Each
cell is a whole number between 0 and 1000000.

One **step** replaces every cell with

```
(4 * self + left + right + up + down) / 8
```

where the division is whole-number division that discards the remainder. Every
value stays at or above zero, so that division is unambiguous.

- A neighbour outside the grid reads the nearest cell inside it: an edge cell's
  missing neighbour is the edge cell itself.
- Every cell of a step is computed from the values the previous step left, not
  from cells the same step has already written.
- `steps` may be 0, which leaves the grid as it was read.

The per-cell update must be computed by a kernel the GPU runs. A loop over the
cells on the processor is not this job, even though it would produce the same
numbers.

Write the finished grid to standard output: `height` lines, each holding the
row's values separated by single spaces. Then one final line:

```
SUM <the total of every cell>
```

Write nothing else to standard output, and exit with status 0.

## Done

The job is finished when the program builds, runs the stencil on the GPU, and
satisfies the contract for any input, not only the one you tried. There are no
tests in the directory; the contract above is the specification.
