## Rule

The Cranelift backend failed to create a code generation module. This is an internal error that occurs during module initialization and is not triggered by user code. It indicates that the backend cannot set up its internal data structures for code generation.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Failed to create module: [internal Cranelift error]
```

## After

This is typically an environment or backend configuration issue. Ensure your compilation environment is properly set up and the backend has sufficient resources.

## Reference

[Code Generation](../reference/codegen.md)
