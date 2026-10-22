## Rule

`miri profile` ran the program to completion and has no samples to report.

The sampler takes a sample each time the program has used another period of CPU
time — a millisecond at the default rate of 1000 Hz, and longer where the kernel
delivers its profiling timer less often, as macOS does. A program that finishes
inside its first period is never sampled. Time spent waiting, on a file, a
sleep or another process, uses no CPU time and is not sampled either.

The samples are written to the record when the program exits, by
returning from `main`, by calling `exit`, or on a runtime trap. A run killed by a
signal, or a runtime that could not start its sampler, leaves no record; the
program's stderr says when the sampler could not start.

An empty table would read as a program that spends its time nowhere, so the
command reports that it has nothing to say instead.

## Messages

- `the program finished before the profiler took a sample`
- `the program finished without writing a profile record`
- `the profile record could not be read: {reason}`

## Help

- `profile a run that spends more CPU time, with a larger input or a higher --hz; the record is written when the program exits, so a program that ends some other way leaves none`

## Before

`hello.mi`:

```miri
fn main()
    println("hello")
```

```sh
miri profile hello.mi
```

The program is over long before a millisecond of CPU time has passed.

## After

Profile a run that does enough work to sample:

```miri
fn main()
    var total = 0
    for i in 0..200000000
        total = (total + i) % 1000003
    println(f"{total}")
```

```sh
miri profile hello.mi
```

## Reference

[Build and Command Line](../reference/build.md)
