## Rule

The program was aborted (SIGABRT). Miri's own runtime reports a failure and exits rather than calling `abort`, so an abort reaching this code came from foreign code the program linked: a C library that aborts instead of returning an error. Validate what you pass across a `runtime` boundary before the call, because an abort leaves no opportunity to recover.

## Before

```miri
runtime "core" fn open_channel(name String) int

fn main()
    let handle = open_channel("")
```

## After

```miri
runtime "core" fn open_channel(name String) int

fn main()
    let name = "events"
    if name.length() > 0
        let handle = open_channel(name)
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
