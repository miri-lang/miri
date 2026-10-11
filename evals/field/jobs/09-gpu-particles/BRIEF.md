# Job — Particles under gravity on the GPU

Write a program in {{LANGUAGE}} that moves a cloud of particles under their
mutual gravity, computing every step on the GPU, and summarizes where they end.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

The first command-line argument is the path of a text file:

```
<count> <steps> <dt>
<x> <y> <vx> <vy>        <- the first particle
...                      <- count lines in all
```

`count` is a whole number between 1 and 2048 and `steps` between 0 and 200; `dt`
and every particle field are decimal numbers. Every particle has mass 1.

One **step** does two things, in this order, for every particle `i`:

1. Its acceleration is the sum, over every other particle `j`, of

   ```
   (pj - pi) / (|pj - pi|^2 + 0.01)^1.5
   ```

   where `p` is a position and `|.|` a length. Every acceleration of a step is
   computed from the positions the step started with.
2. Its velocity grows by its acceleration times `dt`, and then its position
   grows by that new velocity times `dt`.

`steps` may be 0, which leaves the particles where they were read.

Write four lines to standard output, each value with six digits after the
decimal point:

```
CENTER <mean x> <mean y>
MOMENTUM <sum of vx> <sum of vy>
KINETIC <sum of (vx * vx + vy * vy) / 2>
SPREAD <square root of the mean squared distance from the center>
```

Single-precision arithmetic is fine; the values are checked with a small
tolerance that allows for it.

Both parts of a step must be computed by kernels the GPU runs. A loop over the
particles on the processor is not this job, even though it would produce the
same numbers.

Write nothing else to standard output, and exit with status 0.

## Done

The job is finished when the program builds, moves the particles on the GPU,
and satisfies the contract for any input, not only the one you tried. There are
no tests in the directory; the contract above is the specification.
