## Rule

The program was aborted (SIGABRT). Miri's own runtime reports a failure and exits rather than calling `abort`, so an abort reaching this code came from foreign code the program linked: a C library that aborts instead of returning an error. The example binds the C library's `abort` through a `runtime` declaration, which is what such a library does on the inside. Validate what you pass across a `runtime` boundary before the call, because an abort leaves no opportunity to recover.

## Messages

- `terminated by signal {number} ({name})`

## Before

```miri
runtime "core" fn abort()

fn main()
    println("starting")
    abort()
```

## After

```miri
fn main()
    println("starting")
    println("finished without aborting")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
