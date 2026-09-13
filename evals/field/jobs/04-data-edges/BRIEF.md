# Job — Keyed totals

Write a program in {{LANGUAGE}} that totals values by key and answers queries.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

The first command-line argument is the path of a UTF-8 text file with two
sections separated by one empty line:

```
<key> <value>
<key> <value>
...
                      <- one empty line
<key>
<key>
...
```

- A **key** is a run of characters with no space in it. Keys may hold any
  character, not only the Latin alphabet, and two keys are the same key when
  their bytes are the same.
- A **value** is a decimal integer, possibly negative, that fits in a signed
  64-bit integer: `-9223372036854775808` to `9223372036854775807`.
- The second section is a list of keys to answer, one per line.

For each queried key, in the order the queries appear, write one line to
standard output:

- `<key>=<total>` where the total is the sum of every value recorded under that
  key, added **in the order the records appear**.
- `<key>=absent` when the first section records no value under that key.
- `<key>=overflow` when a running total would leave the signed 64-bit range.
  Report the key that way and move on to the next query.

A file with no records and no queries produces no output. Write nothing else to
standard output, and exit with status 0.

## Done

The job is finished when the program builds and satisfies the contract for any
input, not only the one you tried. There are no tests in the directory; the
contract above is the specification.
