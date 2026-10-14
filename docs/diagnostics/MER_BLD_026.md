## Rule

`miri run` or `miri build` was given a file whose only callable code is tests:
it declares at least one `@test` function, and no `main` or top-level statement
that script-mode wrapping would demote into a synthetic one.

That is the correct shape for a test file. A test file holds only declarations
and `miri test` supplies the entry point, so the missing `main` is not a defect
to fix — adding one makes the runner refuse the file. The note says only that
the command used does not run what the file holds: the executable a build
produces from it calls none of its tests.

A note rather than a warning, because nothing about the file is wrong. It is
reported so that a build which produced an executable that runs nothing is not
mistaken for one that produced a working program.

## Messages

- `this file declares '@test' functions and no other entry point, so the executable built from it runs none of them`

## Help

- `run the tests it declares with 'miri test' on this file.`

## Before

```miri
// x_test.mi: @test functions and nothing else, handed to `miri build` or `miri run`
use system.testing

@test
fn test_adds()
    assert(1 + 1 == 2)
```

## After

The file is unchanged; the command is the one that runs what it declares:
`miri test x_test.mi`.

## Reference

[Build and Command Line](../reference/build.md)
