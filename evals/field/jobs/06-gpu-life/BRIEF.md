# Job — Game of Life on the GPU

Write a program in {{LANGUAGE}} that runs Conway's Game of Life on a grid,
computing every generation on the GPU.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

The first command-line argument is the path of a text file:

```
<width> <height> <generations>
<width digits, each 0 or 1>     <- the first row
...                             <- height rows in all
```

`width` and `height` are between 1 and 512; `generations` is between 0 and
1000. A `1` is a live cell and a `0` a dead one. The digits of a row are
separated by single spaces.

One **generation** decides every cell from its eight neighbours:

- A live cell with two or three live neighbours stays alive; any other live
  cell dies.
- A dead cell with exactly three live neighbours becomes alive; any other dead
  cell stays dead.
- The grid does not wrap. A neighbour outside the grid is dead.
- Every cell of a generation is decided from the cells the previous generation
  left, not from cells the same generation has already decided.
- `generations` may be 0, which leaves the grid as it was read.

The per-cell update must be computed by a kernel the GPU runs. A loop over the
cells on the processor is not this job, even though it would produce the same
grid.

Write the final grid to standard output in the input's row format: `height`
lines of `width` digits separated by single spaces. Then one final line:

```
ALIVE <the number of live cells>
```

Write nothing else to standard output, and exit with status 0.

## Done

The job is finished when the program builds, runs the generations on the GPU,
and satisfies the contract for any input, not only the one you tried. There are
no tests in the directory; the contract above is the specification.
