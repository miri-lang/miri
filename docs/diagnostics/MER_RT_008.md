## Rule

An illegal instruction (SIGILL) terminated the program. The processor reached an opcode it does not implement or refuses to run. Miri's own code generation builds for the machine it runs on, so the realistic source is foreign code the program links: a C library built for a wider instruction set than the machine running it, or one that stops itself with a trap instruction (`__builtin_trap` compiles to one on x86-64). The example raises the signal through the C library's `raise` (SIGILL is 4 on Linux and macOS), which ends the program the same way. Rebuild the library for the oldest machine it has to run on, and check what it was doing when it stopped.

## Messages

- `terminated by signal {number} ({name})`

## Before

```miri
runtime "core" fn raise(signal i32) i32

fn main()
    println("starting")
    let status = raise(4)
    println(f"{status}")
```

## After

```miri
fn main()
    println("starting")
    println("finished without a foreign trap")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
