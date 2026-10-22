## Rule

The program `miri profile` was running died before it finished: it stopped on a
runtime trap, was killed by a signal, or ended without an exit status.

A run that died is not a profile of the program, only of the part that ran up
to the fault. The samples that part took are still reported beside this
diagnostic when a runtime trap ended the run, because the trap writes the record
before it exits; a signal, or a trap on a heap the guard found corrupted, leaves
none. The diagnostic after this one names what the run died of, with the code
`miri run` reports for it, and the program's own stderr carries the runtime's
account of where.

A program that finishes with a status of its own, such as `exit(3)`, has not
died: it is profiled, and the command ends with that status.

## Messages

- `the program died while it was profiled: {reason}`

## Help

- `a profile is taken of a run that finishes; 'miri run' on the same file reports the failure where it happens`

## Before

`scores.mi`:

```miri
fn main()
    let scores = [90, 85, 77]
    var i = 3
    println(f"{scores[i]}")
```

```sh
miri profile scores.mi
```

The index is past the end of the array, and the run stops on a trap.

## After

```miri
fn main()
    let scores = [90, 85, 77]
    var i = 2
    println(f"{scores[i]}")
```

Then profile the program that runs to the end.

## Reference

[Build and Command Line](../reference/build.md)
