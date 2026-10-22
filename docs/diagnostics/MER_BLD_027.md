## Rule

`miri profile` was run on a host whose runtime has no sampling profiler.

The profiler interrupts the program on a CPU-time timer and reads the
interrupted instruction and frame pointer out of the signal context. Where to
find them, and which timer counts a process's CPU time, differs by operating
system and processor, and the runtime carries both only for macOS and Linux, on
aarch64 and x86-64. Elsewhere there is nothing to sample with, so the command
refuses before building rather than reporting a run that took no samples.

A program built with `--profile` still runs on such a host: it keeps a frame
pointer in every function and writes its line map, so a profiler the platform
provides can walk its stacks.

## Messages

- `this host ({target}) has no sampling profiler; profiling runs on macOS and Linux, on aarch64 and x86-64`

## Help

- `profile on a macOS or Linux machine with an aarch64 or x86-64 processor`

## Before

`app.mi`:

```miri
fn main()
    var total = 0
    for i in 0..1000000
        total += i
    println(f"{total}")
```

```sh
miri profile app.mi
```

On a host other than macOS or Linux on aarch64 or x86-64, the command has no
sampler to run the program under.

## After

Run the same command on a supported host, or build for the platform's own
profiler:

```sh
miri build app.mi --profile
```

## Reference

[Build and Command Line](../reference/build.md)
