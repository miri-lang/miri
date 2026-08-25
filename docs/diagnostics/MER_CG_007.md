## Rule

The specified code generation backend is not available in this build of the Miri compiler. The Cranelift backend is always available; other backends such as LLVM may not be compiled in. Use the default Cranelift backend or specify a backend that is available.

## Before

```miri
fn main()
    println("hello")
```

Compiled with: `miri build --cpu-backend llvm program.mi`

## After

Either use the default Cranelift backend (omit `--cpu-backend` flag) or verify the backend is available:

```
miri build program.mi
```

## Reference

[Code Generation](../reference/codegen.md)
