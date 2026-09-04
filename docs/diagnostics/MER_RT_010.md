## Rule

A signal terminated the program, and it was not one of the faults that carry their own code. The signal number and name are in the message. The common cause is a signal the host sent rather than a fault the program raised: the out-of-memory killer stopping a process that allocated without bound (SIGKILL), a shutdown request (SIGTERM), or an interrupt from the terminal (SIGINT). Bound the work so the host has no reason to intervene.

## Before

```miri
use system.collections.list

fn main()
    var rows = List<int>()
    var i = 0
    while true
        rows.push(i)
        i = i + 1
```

## After

```miri
use system.collections.list

fn main()
    var rows = List<int>()
    var i = 0
    while i < 1000000
        rows.push(i)
        i = i + 1
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
