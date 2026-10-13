## Rule

A signal terminated the program, and it was not one of the faults that carry their own code. The signal number and name are in the message. The common cause is a signal the host sent rather than a fault the program raised: the out-of-memory killer stopping a process that allocated without bound (SIGKILL), a shutdown request (SIGTERM), or an interrupt from the terminal (SIGINT). The example sends itself SIGTERM (15) through the C library's `raise`, which ends it exactly as a shutdown request would. Bound the work so the host has no reason to intervene.

## Messages

- `terminated by signal {number} ({name})`

## Before

```miri
runtime "core" fn raise(signal i32) i32

fn main()
    let status = raise(15)
    println(f"{status}")
```

## After

```miri
fn main()
    println("finished before any signal arrived")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
