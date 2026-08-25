## Rule

An unexpected internal error occurred during code generation. This error indicates a backend bug or an unsupported combination of features that the backend does not handle correctly. It is not triggered by user code.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Internal codegen error: [internal Cranelift error]
```

## After

This is typically a compiler bug. Report this error along with the program that triggered it, as it may indicate a missing feature or incorrect backend implementation.

## Reference

[Code Generation](../reference/codegen.md)
