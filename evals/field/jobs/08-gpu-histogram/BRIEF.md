# Job — A histogram on the GPU

Write a program in {{LANGUAGE}} that generates a long sequence of values and
counts them into a histogram, generating and counting on the GPU.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

The first command-line argument is the path of a text file of one line:

```
<count> <seed> <bins>
```

`count` is a whole number between 0 and 4000000, `seed` between 0 and 9999, and
`bins` between 1 and 256.

Value number `i`, for `i` from 0 to `count - 1`, is computed with whole-number
arithmetic, where `%` is the remainder and `/` discards it:

```
a = ((i % 40000) * 1597 + seed + 51749) % 40000
b = (a * a + a * 37 + 11) % 40009
c = (b * 613 + 7919) % 40009
d = (c + (i / 40000) * 7919) % 40009
value = d % 1000
```

Every intermediate fits in a signed 32-bit integer. The value is between 0 and
999, and it belongs to bin `value * bins / 1000`.

Write `bins` lines to standard output, one per bin in order, then a final line:

```
BIN <index> <how many values fell in it>
...
TOTAL <count>
```

Both the values and the counting must be computed by kernels the GPU runs. A
loop over the values on the processor is not this job, even though it would
produce the same counts.

Write nothing else to standard output, and exit with status 0.

## Done

The job is finished when the program builds, generates and counts the values on
the GPU, and satisfies the contract for any input, not only the one you tried.
There are no tests in the directory; the contract above is the specification.
