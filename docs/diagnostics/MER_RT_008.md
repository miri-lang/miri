## Rule

An illegal instruction (SIGILL) terminated the program. The processor reached an opcode it does not implement. This happens when a binary is built for a wider instruction set than the machine running it, so the program starts and then dies on the first unsupported instruction. Build for the host, or for the oldest machine the binary has to run on.

## Before

```
miri build main.mi --target-cpu native   # then run on an older machine
```

## After

```
miri build main.mi                        # the portable baseline
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
